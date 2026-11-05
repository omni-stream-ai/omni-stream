use crate::config::Config;
use crate::policy;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub name: String,
    pub runtime: String,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Initialize {
        protocol_version: u32,
    },
    InitializeResult {
        manifest: Manifest,
    },
    Invoke {
        id: String,
        capability: String,
        input: serde_json::Value,
    },
    Result {
        id: String,
        output: serde_json::Value,
    },
    Event {
        event: String,
        payload: serde_json::Value,
    },
    Error {
        id: Option<String>,
        message: String,
    },
}

pub fn validate(manifest: &Manifest) -> Result<(), String> {
    if manifest.id.trim().is_empty() || manifest.version.trim().is_empty() {
        return Err("插件 manifest 缺少 id/version".into());
    }
    validate_id(&manifest.id)?;
    if manifest.capabilities.is_empty() {
        return Err("插件至少需要声明一个 capability".into());
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), String> {
    if !id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err(
            "插件 id 必须以字母或数字开头，且只能包含字母、数字、点、下划线或连字符".into(),
        );
    }
    Ok(())
}

pub struct Registry {
    root: PathBuf,
}
impl Registry {
    pub fn global() -> Self {
        let root = std::env::var_os("OMNISTREAM_HOME")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("plugins");
        Self::new(root)
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub(crate) fn root(&self) -> &std::path::Path {
        &self.root
    }
    pub fn discover(&self) -> Result<Vec<Manifest>, String> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut manifests = Vec::new();
        for entry in std::fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
            let manifest: Manifest = serde_json::from_str(&text)
                .map_err(|e| format!("解析 {} 失败：{e}", path.display()))?;
            validate(&manifest)?;
            manifests.push(manifest);
        }
        manifests.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(manifests)
    }
    pub fn install(&self, manifest: &Manifest) -> Result<PathBuf, String> {
        validate(manifest)?;
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let path = self.root.join(format!("{}.json", manifest.id));
        let data = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
        // Publish complete JSON so concurrent readers never see a partial manifest.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let temporary = self.root.join(format!(
            ".{}-{}-{nonce}.tmp",
            manifest.id,
            std::process::id()
        ));
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&data)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        Ok(path)
    }

    pub fn uninstall(&self, id: &str) -> Result<(), String> {
        validate_id(id)?;
        match std::fs::remove_file(self.root.join(format!("{id}.json"))) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[allow(dead_code)]
pub struct PluginProcess {
    child: std::process::Child,
    pub manifest: Manifest,
}
#[allow(dead_code)]
impl PluginProcess {
    pub fn start(command: &str, args: &[String], cwd: &std::path::Path) -> Result<Self, String> {
        let mut child = Command::new(command)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("启动插件失败：{e}"))?;
        let mut input = child.stdin.take().ok_or("plugin stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("plugin stdout unavailable")?;
        send(
            &mut input,
            &Message::Initialize {
                protocol_version: 1,
            },
        )?;
        let mut reader = BufReader::new(stdout);
        let response = receive(&mut reader)?.ok_or("插件在初始化前退出")?;
        let manifest = match response {
            Message::InitializeResult { manifest } => manifest,
            Message::Error { message, .. } => return Err(message),
            _ => return Err("插件初始化响应类型错误".into()),
        };
        validate(&manifest)?;
        child.stdin = Some(input);
        child.stdout = Some(reader.into_inner());
        Ok(Self { child, manifest })
    }
    pub fn healthy(&mut self) -> bool {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(false)
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.child.kill().map_err(|e| e.to_string())?;
        self.child.wait().map(|_| ()).map_err(|e| e.to_string())
    }
}

#[allow(dead_code)]
pub struct Host<'a> {
    pub config: &'a Config,
    pub manifest: Manifest,
}
#[allow(dead_code)]
impl<'a> Host<'a> {
    pub fn invoke(&self, action: &str, id: &str, input: serde_json::Value) -> Message {
        if policy::requires_approval(self.config, action) {
            return Message::Event {
                event: "approval.required".into(),
                payload: serde_json::json!({"id":id,"action":action,"input":input}),
            };
        }
        if !self
            .manifest
            .actions
            .iter()
            .any(|allowed| allowed == action)
        {
            return Message::Error {
                id: Some(id.into()),
                message: format!("插件未声明动作：{action}"),
            };
        }
        Message::Invoke {
            id: id.into(),
            capability: action.into(),
            input,
        }
    }
}

pub fn send<W: Write>(writer: &mut W, message: &Message) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, message).map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

pub fn receive<R: BufRead>(reader: &mut R) -> Result<Option<Message>, String> {
    let mut line = String::new();
    let count = reader.read_line(&mut line).map_err(|e| e.to_string())?;
    if count == 0 {
        return Ok(None);
    }
    serde_json::from_str(line.trim())
        .map(Some)
        .map_err(|e| format!("插件消息无效：{e}"))
}

pub fn run_stdio_agent(
    command: &str,
    args: &[String],
    cwd: &std::path::Path,
    goal: &str,
) -> Result<String, String> {
    if !cwd.is_dir() {
        return Err(format!("工作目录不存在：{}", cwd.display()));
    }
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 ACP agent 失败：{e}"))?;
    let mut input = child.stdin.take().ok_or("ACP stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("ACP stdout unavailable")?;
    send(
        &mut input,
        &Message::Initialize {
            protocol_version: 1,
        },
    )?;
    send(
        &mut input,
        &Message::Invoke {
            id: "run".into(),
            capability: "agent.execute".into(),
            input: serde_json::json!({"goal":goal}),
        },
    )?;
    drop(input);
    let mut reader = BufReader::new(stdout);
    let mut output = Vec::new();
    loop {
        match receive(&mut reader)? {
            Some(Message::Result { output: value, .. }) => {
                output.push(value.to_string());
                break;
            }
            Some(Message::Event { event, payload }) => output.push(format!("{event}: {payload}")),
            Some(Message::Error { message, .. }) => {
                let _ = child.kill();
                return Err(message);
            }
            Some(_) => (),
            None => break,
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!(
            "ACP agent 退出码 {:?}\n{}",
            status.code(),
            output.join("\n")
        ));
    }
    Ok(output.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_round_trip() {
        let msg = Message::Event {
            event: "run.progress".into(),
            payload: serde_json::json!({"percent": 20}),
        };
        let mut bytes = Vec::new();
        send(&mut bytes, &msg).unwrap();
        let parsed = receive(&mut std::io::Cursor::new(bytes)).unwrap().unwrap();
        assert!(matches!(parsed, Message::Event { .. }));
    }

    #[test]
    fn host_turns_risky_actions_into_approval_events() {
        let config: crate::config::Config = toml::from_str("approvals=[\"deploy\"]").unwrap();
        let manifest = Manifest {
            id: "p".into(),
            version: "1".into(),
            name: "p".into(),
            runtime: "rust".into(),
            capabilities: vec!["deploy".into()],
            permissions: vec![],
            actions: vec!["deploy".into()],
        };
        let message = Host {
            config: &config,
            manifest,
        }
        .invoke("deploy", "1", serde_json::json!({}));
        assert!(matches!(message, Message::Event { event, .. } if event == "approval.required"));
    }

    #[test]
    fn stdio_agent_returns_result_message() {
        let output = run_stdio_agent(
            "sh",
            &[
                "-c".into(),
                "printf '%s\\n' '{\"type\":\"result\",\"id\":\"run\",\"output\":{\"ok\":true}}'"
                    .into(),
            ],
            std::path::Path::new("."),
            "hello",
        )
        .unwrap();
        assert!(output.contains("ok"));
    }
    #[test]
    fn registry_installs_and_discovers_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            id: "demo".into(),
            version: "1.0.0".into(),
            name: "Demo".into(),
            runtime: "external".into(),
            capabilities: vec!["x".into()],
            permissions: vec![],
            actions: vec![],
        };
        let registry = Registry::new(dir.path());
        registry.install(&manifest).unwrap();
        let found = registry.discover().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "demo");
    }

    #[test]
    fn registry_rejects_paths_and_preserves_unrelated_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::new(dir.path());
        let mut manifest = Manifest {
            id: "custom".into(),
            version: "1".into(),
            name: "Custom".into(),
            runtime: "external".into(),
            capabilities: vec!["custom.read".into()],
            permissions: vec![],
            actions: vec![],
        };
        registry.install(&manifest).unwrap();
        for id in ["../escaped", "/tmp/escaped", "a/b", "a\\b", "..", "", " a"] {
            manifest.id = id.into();
            assert!(registry.install(&manifest).is_err());
            assert!(registry.uninstall(id).is_err());
        }
        registry.uninstall("slack").unwrap();
        assert_eq!(registry.discover().unwrap()[0].id, "custom");
    }
}
