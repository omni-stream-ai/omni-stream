//! Opt-in remote delivery of the existing, redacted notification feed.
mod credentials;
mod reads;
mod sender;
mod storage;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub use storage::schema;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub firebase_project: Option<String>,
    pub credentials_file: Option<std::path::PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            firebase_project: None,
            credentials_file: None,
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn random() -> Result<String, String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| "无法生成推送凭据")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn open(path: &Path) -> Result<Connection, String> {
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    Ok(db)
}

#[cfg(test)]
fn api(
    path: &Path,
    settings: &Settings,
    method: &str,
    route: &str,
    body: &[u8],
) -> Result<Value, String> {
    api_with_client(path, settings, method, route, body, None)
}

fn check_client(db: &Connection, client: Option<&str>) -> Result<(), String> {
    if let Some(id) = client {
        let active: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM client_authorizations WHERE id=?1 AND status='approved')", [id], |r|r.get(0)).map_err(|e|e.to_string())?;
        if !active {
            return Err("当前客户端已被撤销授权".into());
        }
    }
    Ok(())
}

pub fn api_with_client(
    path: &Path,
    settings: &Settings,
    method: &str,
    route: &str,
    body: &[u8],
    client: Option<&str>,
) -> Result<Value, String> {
    if route == "/push/status" && method == "GET" {
        return Ok(
            json!({"enabled":settings.enabled,"android":settings.enabled && sender::fcm_ready(settings),
            "web":settings.enabled,"vapid_public_key":if settings.enabled { Some(sender::vapid(path)?.1) } else {None}}),
        );
    }
    if method != "POST" {
        return Err("推送接口不存在".into());
    }
    if body.len() > 16384 {
        return Err("推送请求过大".into());
    }
    let input: storage::Input = serde_json::from_slice(body).map_err(|_| "推送请求格式无效")?;
    let mut db = open(path)?;
    if route == "/push/devices/register" {
        if !settings.enabled {
            return Err("Core 未启用远程推送".into());
        }
        if input.transport.as_deref() == Some("android") && !sender::fcm_ready(settings) {
            return Err("Core 未配置 Android 推送凭据".into());
        }
        storage::register_with_client(&mut db, input, client)
    } else {
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        check_client(&tx, client)?;
        let result = storage::device_api(&tx, route, input)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }
}

pub fn cli(args: &[String]) -> Option<Result<(), String>> {
    if args.first().map(String::as_str) != Some("push") {
        return None;
    }
    Some((|| {
        let path = crate::state_path();
        crate::storage::Store::open(&path)?;
        let db = open(&path)?;
        match args.get(1).map(String::as_str) {
            Some("devices") if args.len() == 2 => println!("{}", storage::list(&db)?),
            Some("revoke") if args.len() == 3 => {
                storage::revoke(&db, &args[2])?;
                println!("revoked");
            }
            _ => return Err("用法：omnistream push devices | revoke <device-id>".into()),
        }
        Ok(())
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_provides_push_by_default_and_can_explicitly_disable_it() {
        assert!(Settings::default().enabled);
        assert!(
            serde_json::from_value::<Settings>(json!({}))
                .unwrap()
                .enabled
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        crate::storage::Store::open(&path).unwrap();
        let enabled = api(&path, &Settings::default(), "GET", "/push/status", b"").unwrap();
        assert_eq!(enabled["web"], true);
        assert!(enabled["vapid_public_key"].is_string());
        let disabled = Settings {
            enabled: false,
            ..Settings::default()
        };
        let status = api(&path, &disabled, "GET", "/push/status", b"").unwrap();
        assert_eq!(
            status,
            json!({"enabled":false,"android":false,"web":false,"vapid_public_key":null})
        );
        assert!(api(&path, &disabled, "POST", "/push/devices/register", b"{}").is_err());
    }
}

pub fn start(settings: Settings) -> Result<(), String> {
    if !settings.enabled {
        return Ok(());
    }
    let path = crate::state_path();
    let key = sender::vapid(&path)?.0;
    let db = open(&path)?;
    // A crash after claiming a send has an unknown delivery outcome. Do not replay it.
    storage::recover(&db)?;
    let mut sender = sender::Sender::new(settings, key)?;
    std::thread::Builder::new().name("remote-push".into()).spawn(move || {
        loop {
            let result = (|| {
                let mut db = open(&path)?;
                let slack = crate::plugin_marketplace::slack_connection()?;
                storage::dispatch(&mut db, slack.as_ref(), |device, payload| sender.send(device, payload))
            })();
            if result.is_err() {
                // Database errors are also explicit failures, never an automatic replay loop.
                if let Ok(db) = open(&path) { let _ = db.execute("UPDATE push_devices SET state='paused',error='storage_error' WHERE state='active'", []); }
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    }).map_err(|e| e.to_string())?;
    Ok(())
}
