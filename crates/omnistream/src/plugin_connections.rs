//! Private credentials; only the explicit public summary may cross the Core API.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    #[default]
    Token,
    SlackUser,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenScope {
    #[default]
    Related,
    All,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub token: String,
    #[serde(default)]
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub app_token: Option<String>,
    #[serde(default)]
    pub listen: bool,
    #[serde(default)]
    pub listen_scope: ListenScope,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub generation: String,
    pub enabled: bool,
    pub allow_writes: bool,
    pub account: Option<String>,
    pub verified_at: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Summary {
    pub status: &'static str,
    pub auth_mode: AuthMode,
    pub app_token_configured: bool,
    pub listen: bool,
    pub listen_scope: ListenScope,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub configured: bool,
    pub enabled: bool,
    pub allow_writes: bool,
    pub account: Option<String>,
    pub verified_at: Option<u64>,
    pub last_error: Option<String>,
}
impl Summary {
    pub fn from_connection(connection: Option<&Connection>) -> Self {
        Self {
            auth_mode: connection.map(|c| c.auth_mode).unwrap_or_default(),
            app_token_configured: connection.is_some_and(|c| c.app_token.is_some()),
            listen: connection.is_some_and(|c| c.listen),
            listen_scope: connection.map(|c| c.listen_scope).unwrap_or_default(),
            user_id: connection.and_then(|c| c.user_id.clone()),
            team_id: connection.and_then(|c| c.team_id.clone()),
            status: match connection {
                None => "unconfigured",
                Some(c) if !c.enabled => "disabled",
                Some(c) if c.last_error.is_some() => "error",
                Some(_) => "connected",
            },
            configured: connection.is_some(),
            enabled: connection.is_some_and(|c| c.enabled),
            allow_writes: connection.is_some_and(|c| c.allow_writes),
            account: connection.and_then(|c| c.account.clone()),
            verified_at: connection.and_then(|c| c.verified_at),
            last_error: connection.and_then(|c| c.last_error.clone()),
        }
    }
}

pub struct Store {
    root: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_connections_default_to_token_mode_and_user_mode_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_owned());
        std::fs::write(dir.path().join("slack.json"), r#"{"token":"legacy-bot","enabled":true,"allow_writes":false,"account":"bot","verified_at":1,"last_error":null}"#).unwrap();
        let mut connection = store.load("slack").unwrap().unwrap();
        assert!(connection.auth_mode == AuthMode::Token);
        connection.auth_mode = AuthMode::SlackUser;
        connection.token = "xoxp-private-fixture".into();
        store.save("slack", &connection).unwrap();
        let reopened = Store::new(dir.path().to_owned())
            .load("slack")
            .unwrap()
            .unwrap();
        assert!(reopened.auth_mode == AuthMode::SlackUser);
        let summary = serde_json::to_value(Summary::from_connection(Some(&reopened))).unwrap();
        assert_eq!(summary["auth_mode"], "slack_user");
        assert!(!summary.to_string().contains("xoxp-"));
    }
    #[test]
    fn plugin_credentials_are_private_atomic_and_absent_from_summaries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("connections");
        let store = Store::new(root.clone());
        let mut connection = Connection {
            generation: "first".into(),
            token: "private-fixture-token".into(),
            auth_mode: AuthMode::Token,
            app_token: None,
            listen: false,
            listen_scope: ListenScope::Related,
            user_id: None,
            team_id: None,
            enabled: true,
            allow_writes: false,
            account: Some("tester".into()),
            verified_at: Some(1),
            last_error: None,
        };
        store.save("github", &connection).unwrap();
        assert_eq!(
            store.load("github").unwrap().unwrap().token,
            connection.token
        );
        assert!(
            !serde_json::to_string(&Summary::from_connection(Some(&connection)))
                .unwrap()
                .contains(&connection.token)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(root.join("github.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        connection.token = "replacement-fixture-token".into();
        store.save("github", &connection).unwrap();
        assert_eq!(
            store.load("github").unwrap().unwrap().token,
            connection.token
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert!(store.remove("../github").is_err());
        store.remove("github").unwrap();
        store.remove("github").unwrap();
        assert!(store.load("github").unwrap().is_none());
        std::fs::write(root.join("slack.json"), "bad json").unwrap();
        assert!(store.load("slack").is_err());
    }
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !["slack", "github", "notion", "figma"].contains(&id) {
            return Err("未知插件".into());
        }
        Ok(self.root.join(format!("{id}.json")))
    }
    pub fn load(&self, id: &str) -> Result<Option<Connection>, String> {
        let path = self.path(id)?;
        match std::fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data)
                .map(Some)
                .map_err(|_| "插件连接配置损坏，请断开连接后重新配置".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err("无法读取插件连接配置".into()),
        }
    }
    pub fn save(&self, id: &str, connection: &Connection) -> Result<(), String> {
        let path = self.path(id)?;
        std::fs::create_dir_all(&self.root).map_err(|_| "无法创建插件凭据目录")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "无法保护插件凭据目录")?;
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = self
            .root
            .join(format!(".{id}-{}-{nonce}.tmp", std::process::id()));
        let result = (|| -> Result<(), String> {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|_| "无法创建插件凭据文件")?;
            let data = serde_json::to_vec(connection).map_err(|_| "无法保存插件凭据")?;
            file.write_all(&data)
                .and_then(|_| file.sync_all())
                .map_err(|_| "无法保存插件凭据")?;
            std::fs::rename(&temporary, &path).map_err(|_| "无法替换插件凭据文件")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path(id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("无法删除插件凭据".into()),
        }
    }
}
