use serde::Deserialize;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize, Default)]
pub struct Config {
    pub model_provider: Option<String>,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub model_reasoning_efforts: Option<Vec<String>>,
    #[serde(default)]
    pub model_providers: HashMap<String, ModelProviderConfig>,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
    #[serde(default)]
    pub approvals: Vec<String>,
    #[serde(default)]
    pub auto_approve: Vec<String>,
    pub approval_mode: Option<ApprovalMode>,
    pub max_concurrency: Option<usize>,
    #[serde(skip)]
    pub message_attachments: Vec<serde_json::Value>,
    #[serde(skip)]
    pub plugin_setup: bool,
    /// Internal execution policy for callers that consume an Agent's result directly.
    #[serde(skip)]
    pub agent_stdout_only: bool,
    #[serde(skip)]
    pub agent_mcp_servers: serde_json::Value,
    #[serde(skip)]
    credentials: Arc<Mutex<HashMap<String, (Instant, String)>>>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalMode {
    Default,
    Safe,
    All,
}

impl Default for ApprovalMode {
    fn default() -> Self {
        Self::Default
    }
}

impl Config {
    pub fn approval_mode(&self) -> ApprovalMode {
        self.approval_mode.unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub command: String,
    pub config_file: Option<PathBuf>,
    pub protocol: Option<String>,
    /// Accepted for old configurations; Agent runs no longer have a time limit.
    #[allow(dead_code)]
    pub timeout_ms: Option<u64>,
    pub retries: Option<u32>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub plugin_transport: crate::agent_plugins::PluginTransport,
    #[serde(default)]
    pub mcp_args: Vec<String>,
    #[serde(skip)]
    pub plugin_tools: crate::agent_plugins::Snapshot,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelProviderConfig {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
    pub reasoning_efforts: Option<Vec<String>>,
    pub auth: Option<AuthCommand>,
    #[serde(default)]
    pub models: Vec<ModelOption>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ModelOption {
    pub id: String,
}

fn default_reasoning_efforts() -> Vec<String> {
    vec!["low".into(), "medium".into(), "high".into()]
}

impl Config {
    pub fn reasoning_efforts(&self, provider_id: &str) -> Vec<String> {
        self.model_providers
            .get(provider_id)
            .and_then(|provider| provider.reasoning_efforts.clone())
            .unwrap_or_else(|| {
                self.model_reasoning_efforts
                    .clone()
                    .filter(|levels| !levels.is_empty())
                    .unwrap_or_else(default_reasoning_efforts)
            })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthCommand {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub refresh_interval_ms: Option<u64>,
}

impl Config {
    pub fn load(project_dir: Option<&Path>) -> Result<Self, String> {
        let mut paths = Vec::new();
        if let Some(home) = env::var_os("OMNISTREAM_HOME") {
            paths.push(PathBuf::from(home).join("config.toml"));
        } else if let Some(home) = env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".omnistream/config.toml"));
        }
        if let Some(dir) = project_dir {
            paths.push(dir.join(".omnistream/config.toml"));
        }
        let mut config = Config::default();
        for path in paths.into_iter().filter(|path| path.exists()) {
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
            let mut overlay: Config =
                toml::from_str(&text).map_err(|e| format!("解析 {} 失败: {e}", path.display()))?;
            for agent in overlay.agents.values_mut() {
                agent.resolve_config_file(path.parent().unwrap_or(Path::new(".")))?;
            }
            if overlay.model_provider.is_some() {
                config.model_provider = overlay.model_provider;
            }
            if overlay.model.is_some() {
                config.model = overlay.model;
            }
            if overlay.model_reasoning_effort.is_some() {
                config.model_reasoning_effort = overlay.model_reasoning_effort;
            }
            if overlay.model_reasoning_efforts.is_some() {
                config.model_reasoning_efforts = overlay.model_reasoning_efforts;
            }
            config.model_providers.extend(overlay.model_providers);
            config.agents.extend(overlay.agents);
            if overlay.approval_mode.is_some() {
                config.approval_mode = overlay.approval_mode;
            }
            if !overlay.approvals.is_empty() {
                config.approvals = overlay.approvals;
            }
        }
        Ok(config)
    }

    pub fn selected_provider(&self) -> Option<&ModelProviderConfig> {
        self.model_provider
            .as_deref()
            .and_then(|name| self.model_providers.get(name))
    }

    pub fn summary(&self) -> String {
        match (&self.model_provider, &self.model, self.selected_provider()) {
            (Some(provider), Some(model), Some(details)) => format!(
                "provider={provider} ({}), model={model}, effort={}, url={}, wire_api={}",
                details.name.as_deref().unwrap_or(provider),
                self.model_reasoning_effort.as_deref().unwrap_or("medium"),
                details.base_url.as_deref().unwrap_or("未配置"),
                details.wire_api.as_deref().unwrap_or("responses")
            ),
            _ => "未配置模型".into(),
        }
    }

    pub fn api_key(&self) -> Result<Option<String>, String> {
        let Some(auth) = self.selected_provider().and_then(|p| p.auth.as_ref()) else {
            return Ok(None);
        };
        let cache_id = format!(
            "{}\0{}\0{:?}",
            self.model_provider.as_deref().unwrap_or(""),
            auth.command,
            auth.args
        );
        let interval = Duration::from_millis(auth.refresh_interval_ms.unwrap_or(0));
        if let Some((at, key)) = self
            .credentials
            .lock()
            .map_err(|_| "认证缓存不可用")?
            .get(&cache_id)
        {
            if !interval.is_zero() && at.elapsed() < interval {
                return Ok(Some(key.clone()));
            }
        }
        let output = auth.execute()?;
        if !output.status.success() {
            return Err(format!("认证命令退出码 {:?}", output.status.code()));
        }
        let key = String::from_utf8(output.stdout)
            .map_err(|_| "认证命令输出不是 UTF-8")?
            .trim()
            .to_string();
        if key.is_empty() {
            return Err("认证命令没有返回 API key".into());
        }
        if key.contains(['\r', '\n']) {
            return Err("认证命令必须只输出一行 API key".into());
        }
        self.credentials
            .lock()
            .map_err(|_| "认证缓存不可用")?
            .insert(cache_id, (Instant::now(), key.clone()));
        Ok(Some(key))
    }
}

impl AgentConfig {
    fn resolve_config_file(&mut self, base: &Path) -> Result<(), String> {
        if let Some(file) = &self.config_file {
            if self.protocol.as_deref() != Some("app-server") {
                return Err("config_file 仅支持 app-server 适配器".into());
            }
            if file.as_os_str().is_empty() {
                return Err("config_file 不能为空".into());
            }
            let file = match file.to_str() {
                Some(value) if value.starts_with("~/") => {
                    PathBuf::from(env::var_os("HOME").ok_or("展开 config_file 需要 HOME")?)
                        .join(&value[2..])
                }
                _ if file.is_relative() => base.join(file),
                _ => file.clone(),
            };
            self.config_file = Some(if file.is_absolute() {
                file
            } else {
                env::current_dir().map_err(|e| e.to_string())?.join(file)
            });
        }
        Ok(())
    }

    pub fn app_server_args(&self) -> Result<Vec<String>, String> {
        let mut args = if self.args.is_empty() {
            vec!["app-server".into()]
        } else {
            self.args.clone()
        };
        let Some(path) = &self.config_file else {
            return Ok(args);
        };
        let text = fs::read_to_string(path)
            .map_err(|e| format!("读取 Agent config_file {} 失败：{e}", path.display()))?;
        // Parser diagnostics can contain credentials from source lines; do not echo them.
        let table: toml::Table = toml::from_str(&text)
            .map_err(|_| format!("Agent config_file {} 不是有效 TOML", path.display()))?;
        let mut overrides = Vec::new();
        for (key, value) in table {
            if key.is_empty()
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err("config_file 顶层配置键必须为字母、数字、下划线或连字符".into());
            }
            overrides.push("-c".into());
            overrides.push(format!("{key}={value}"));
        }
        let index = args
            .iter()
            .position(|arg| arg == "app-server")
            .ok_or("使用 config_file 时 args 必须包含 app-server 子命令")?
            + 1;
        args.splice(index..index, overrides);
        Ok(args)
    }
}

#[cfg(test)]
mod agent_file_tests {
    use super::*;
    #[test]
    fn file_values_roundtrip_and_args_override_them() {
        let dir = tempfile::tempdir().unwrap();
        let source = r#"
model = "from-file"
enabled = true
limit = 42
empty = {}
names = ["a b", "quote\"", "$()"]
[[tools]]
name = "one"
[model_providers."with.dot"]
name = "provider"
[model_providers."with.dot".auth]
command = "secret-helper"
args = ["fetch", "key"]
"#;
        fs::write(dir.path().join("profile.toml"), source).unwrap();
        let mut agent: AgentConfig = toml::from_str(
            r#"command="codex"
protocol="app-server"
config_file="profile.toml"
args=["app-server", "-c", 'model="explicit"']"#,
        )
        .unwrap();
        agent.resolve_config_file(dir.path()).unwrap();
        assert_eq!(
            agent.config_file.as_deref(),
            Some(dir.path().join("profile.toml").as_path())
        );
        let args = agent.app_server_args().unwrap();
        assert_eq!(&args[args.len() - 2..], ["-c", "model=\"explicit\""]);
        let values = args[1..args.len() - 2]
            .chunks_exact(2)
            .map(|pair| {
                assert_eq!(pair[0], "-c");
                pair[1].clone()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            toml::from_str::<toml::Table>(&values).unwrap(),
            toml::from_str::<toml::Table>(source).unwrap()
        );
        fs::write(dir.path().join("profile.toml"), "secret = INVALID_SECRET").unwrap();
        let error = agent.app_server_args().unwrap_err();
        assert!(!error.contains("INVALID_SECRET"));
        agent.config_file = Some(dir.path().join("missing.toml"));
        assert!(agent.app_server_args().is_err());
        agent.protocol = Some("cli".into());
        assert!(agent.resolve_config_file(dir.path()).is_err());
    }
}

impl AuthCommand {
    fn execute(&self) -> Result<std::process::Output, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let child = tokio::process::Command::new(&self.command)
                .args(&self.args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| format!("认证命令启动失败：{e}"))?;
            tokio::time::timeout(
                Duration::from_millis(self.timeout_ms.unwrap_or(15_000)),
                child.wait_with_output(),
            )
            .await
            .map_err(|_| "认证命令超时".to_string())?
            .map_err(|e| format!("认证命令失败：{e}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_timeout_is_enforced() {
        let auth = AuthCommand {
            command: "sleep".into(),
            args: vec!["5".into()],
            timeout_ms: Some(25),
            refresh_interval_ms: Some(0),
        };
        let start = Instant::now();
        assert!(auth.execute().unwrap_err().contains("超时"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn parses_requested_provider_shape() {
        let config: Config = toml::from_str(
            r#"
model_provider = "deepseek"
model = "deepseek-flash"
model_reasoning_effort = "low"
[model_providers.deepseek]
name = "deepseek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
[model_providers.deepseek.auth]
command = "pass"
args = ["show", "codex/deepseek-api-key"]
timeout_ms = 15000
refresh_interval_ms = 0
"#,
        )
        .unwrap();
        assert_eq!(
            config
                .selected_provider()
                .unwrap()
                .auth
                .as_ref()
                .unwrap()
                .args
                .len(),
            2
        );
        assert!(config.summary().contains("effort=low"));
    }
}
