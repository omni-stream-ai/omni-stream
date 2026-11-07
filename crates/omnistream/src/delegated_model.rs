//! Request-scoped model configuration. Credentials are resolved only when the
//! App Server process starts and are never part of args, prompts or scope IDs.
use crate::config::Config;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fmt, process::Command, sync::Arc};

pub const PROVIDER: &str = "omnistream_inherited";
pub const KEY_ENV: &str = "OMNISTREAM_DELEGATED_API_KEY";

#[derive(Clone)]
pub struct SelectedModel {
    config: Config,
}
impl fmt::Debug for SelectedModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectedModel")
            .field("provider", &self.config.model_provider)
            .field("model", &self.config.model)
            .field("reasoning", &self.config.model_reasoning_effort)
            .finish_non_exhaustive()
    }
}
impl SelectedModel {
    pub fn capture(config: &Config) -> Option<Arc<Self>> {
        if config.model_provider.is_none() && config.model.is_none() {
            return None;
        }
        let mut config = config.clone();
        config.agents.clear();
        config.message_attachments.clear();
        let selected = config.model_provider.clone();
        config
            .model_providers
            .retain(|id, _| Some(id) == selected.as_ref());
        Some(Arc::new(Self { config }))
    }

    pub fn identity(&self) -> String {
        let provider = self.config.selected_provider();
        let auth = provider
            .and_then(|p| p.auth.as_ref())
            .map(|auth| json!([auth.command, auth.args]));
        // Hash authentication *references*, never resolve or hash the secret.
        format!(
            "{:x}",
            Sha256::digest(
                json!([
                    self.config.model_provider,
                    self.config.model,
                    self.effort(),
                    provider.and_then(|p| p.base_url.as_ref()),
                    provider
                        .and_then(|p| p.wire_api.as_deref())
                        .unwrap_or("responses"),
                    auth
                ])
                .to_string()
                .as_bytes()
            )
        )
    }

    pub fn model(&self) -> Result<&str, String> {
        self.config
            .model
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "委派模型未配置".into())
    }
    pub fn effort(&self) -> &str {
        self.config
            .model_reasoning_effort
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("medium")
    }

    fn overrides(&self) -> Result<Vec<String>, String> {
        let model = self.model()?;
        let provider = self
            .config
            .selected_provider()
            .ok_or("委派模型 provider 未配置")?;
        if provider.wire_api.as_deref().unwrap_or("responses") != "responses" {
            return Err("Codex App Server 继承模型仅支持 responses 接口；当前 provider 不兼容，未回退到 Codex 自身配置".into());
        }
        let base = provider
            .base_url
            .as_deref()
            .ok_or("委派模型缺少 base_url")?;
        let url = reqwest::Url::parse(base).map_err(|_| "委派模型 base_url 无效")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("委派模型 base_url 须为不含凭据、查询参数和片段的 HTTP(S) 地址".into());
        }
        if provider.auth.is_none() {
            return Err("委派模型缺少认证命令，未使用 Codex 登录凭据".into());
        }
        let providers = json!({(PROVIDER):{
            "name":"OmniStream selected provider", "base_url":base,
            "wire_api":"responses", "env_key":KEY_ENV,
            "requires_openai_auth":false, "supports_websockets":false
        }});
        [
            ("model_provider", json!(PROVIDER)),
            ("model", json!(model)),
            ("model_reasoning_effort", json!(self.effort())),
            ("model_providers", providers),
        ]
        .into_iter()
        .map(|(key, value)| {
            let value = toml::Value::try_from(value).map_err(|_| "无法生成委派模型配置")?;
            Ok(format!("{key}={value}"))
        })
        .collect()
    }

    pub fn configure(&self, command: &mut Command) -> Result<(), String> {
        let overrides = self.overrides()?;
        let key = self.config.api_key()?.ok_or("委派模型未返回认证凭据")?;
        for value in overrides {
            command.arg("-c").arg(value);
        }
        command.env(KEY_ENV, key);
        Ok(())
    }

    pub fn thread_params(&self, params: &mut Value) -> Result<(), String> {
        params["model"] = json!(self.model()?);
        params["modelProvider"] = json!(PROVIDER);
        params["config"] = json!({"model_reasoning_effort":self.effort()});
        Ok(())
    }
}

pub fn inherit(target: &mut Config, source: &Config) {
    let selected = SelectedModel::capture(source);
    for agent in target.agents.values_mut() {
        if agent.protocol.as_deref() == Some("app-server") {
            agent.inherited_model = selected.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(wire: &str) -> Config {
        toml::from_str(&format!(
            r#"
model_provider = "test"
model = "chosen-model"
model_reasoning_effort = "high"
[model_providers.test]
base_url = "http://127.0.0.1:1234/v1"
wire_api = "{wire}"
[model_providers.test.auth]
command = "printf"
args = ["fixture-private-api-key"]
[agents.fixture]
command = "codex"
protocol = "app-server"
"#
        ))
        .unwrap()
    }
    #[test]
    fn credentials_only_enter_child_environment_and_overrides_are_valid_toml() {
        let model = SelectedModel::capture(&config("responses")).unwrap();
        let mut command = Command::new("codex");
        command.args(["app-server", "-c", "model=\"native-model\""]);
        model.configure(&mut command).unwrap();
        let args: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(!args.join(" ").contains("fixture-private-api-key"));
        assert!(!format!("{model:?}").contains("fixture-private-api-key"));
        let mut effective = toml::Table::new();
        for pair in args[1..].chunks(2) {
            assert_eq!(pair[0], "-c");
            effective.extend(toml::from_str::<toml::Table>(pair[1]).unwrap());
        }
        assert_eq!(effective["model"].as_str(), Some("chosen-model"));
        assert_eq!(effective["model_provider"].as_str(), Some(PROVIDER));
        assert_eq!(
            effective["model_providers"][PROVIDER]["env_key"].as_str(),
            Some(KEY_ENV)
        );
        assert_eq!(
            command
                .get_envs()
                .find(|(name, _)| *name == KEY_ENV)
                .unwrap()
                .1
                .unwrap(),
            "fixture-private-api-key"
        );
        let mut params = json!({});
        model.thread_params(&mut params).unwrap();
        assert_eq!(params["model"], "chosen-model");
        assert_eq!(params["config"]["model_reasoning_effort"], "high");
    }
    #[test]
    fn incompatible_provider_and_missing_auth_fail_before_launch_without_fallback() {
        let mut config = config("chat_completions");
        config
            .model_providers
            .get_mut("test")
            .unwrap()
            .auth
            .as_mut()
            .unwrap()
            .command = "must-not-run".into();
        let model = SelectedModel::capture(&config).unwrap();
        let mut command = Command::new("must-not-run");
        assert!(model
            .configure(&mut command)
            .unwrap_err()
            .contains("仅支持 responses"));
        assert_eq!(command.get_envs().count(), 0);
        config.model_providers.get_mut("test").unwrap().wire_api = Some("responses".into());
        config.model_providers.get_mut("test").unwrap().auth = None;
        assert!(SelectedModel::capture(&config)
            .unwrap()
            .configure(&mut command)
            .unwrap_err()
            .contains("缺少认证命令"));
    }
    #[test]
    fn scopes_include_selected_model_provider_endpoint_effort_and_auth_reference() {
        let base = config("responses");
        let mut native = base.agents["fixture"].clone();
        let old_scope = crate::agent_host::scope("owner", "/workspace", &native);
        native.inherited_model = SelectedModel::capture(&base);
        let scope = crate::agent_host::scope("owner", "/workspace", &native);
        assert_ne!(old_scope, scope);
        for change in 0..4 {
            let mut changed = base.clone();
            match change {
                0 => changed.model = Some("different-model".into()),
                1 => changed.model_reasoning_effort = Some("low".into()),
                2 => {
                    changed.model_providers.get_mut("test").unwrap().base_url =
                        Some("http://127.0.0.1:4321/v1".into())
                }
                _ => {
                    changed
                        .model_providers
                        .get_mut("test")
                        .unwrap()
                        .auth
                        .as_mut()
                        .unwrap()
                        .args = vec!["other-reference".into()]
                }
            }
            native.inherited_model = SelectedModel::capture(&changed);
            assert_ne!(
                scope,
                crate::agent_host::scope("owner", "/workspace", &native)
            );
        }
        assert!(SelectedModel::capture(&Config::default()).is_none());
    }
    #[test]
    fn a_snapshot_keeps_its_selection_while_credentials_can_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("credential");
        std::fs::write(&path, "first-fixture-key").unwrap();
        let mut source = config("responses");
        let auth = source
            .model_providers
            .get_mut("test")
            .unwrap()
            .auth
            .as_mut()
            .unwrap();
        auth.command = "cat".into();
        auth.args = vec![path.to_string_lossy().into_owned()];
        let captured = SelectedModel::capture(&source).unwrap();
        let identity = captured.identity();
        source.model = Some("changed-later".into());
        let mut first = Command::new("fixture");
        captured.configure(&mut first).unwrap();
        std::fs::write(&path, "rotated-fixture-key").unwrap();
        let mut second = Command::new("fixture");
        captured.configure(&mut second).unwrap();
        assert_eq!(captured.model().unwrap(), "chosen-model");
        assert_eq!(captured.identity(), identity);
        assert_ne!(
            first.get_envs().collect::<Vec<_>>(),
            second.get_envs().collect::<Vec<_>>()
        );
    }
}
