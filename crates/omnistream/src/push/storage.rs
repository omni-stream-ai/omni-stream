use super::sender;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn schema(db: &Connection) -> Result<(), String> {
    super::reads::schema(db)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS push_devices(
            id TEXT PRIMARY KEY,secret_hash TEXT NOT NULL,transport TEXT NOT NULL,
            target TEXT NOT NULL,destination TEXT NOT NULL UNIQUE,server TEXT NOT NULL,locale TEXT NOT NULL,
            cursor INTEGER NOT NULL,instance TEXT NOT NULL,state TEXT NOT NULL DEFAULT 'active',
            error TEXT,inflight INTEGER,revision INTEGER NOT NULL DEFAULT 1);")
        .map_err(|e| e.to_string())
}
fn hash(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}
fn error(e: rusqlite::Error) -> String {
    e.to_string()
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Input {
    pub id: String,
    pub secret: String,
    pub transport: Option<String>,
    pub target: Option<Value>,
    pub server: Option<String>,
    pub locale: Option<String>,
    pub activate: bool,
    pub read: Option<super::reads::Receipt>,
}
fn validate_identity(input: &Input) -> Result<(), String> {
    if input.id.len() < 16
        || input.id.len() > 100
        || !input
            .id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || URL_SAFE_NO_PAD
            .decode(&input.secret)
            .map_or(true, |bytes| bytes.len() != 32)
    {
        return Err("设备凭据无效".into());
    }
    Ok(())
}
fn authorized(db: &Connection, input: &Input) -> Result<bool, String> {
    validate_identity(input)?;
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM push_devices WHERE id=?1 AND secret_hash=?2)",
        params![input.id, hash(&input.secret)],
        |r| r.get(0),
    )
    .map_err(error)
}
#[cfg(test)]
pub fn register(db: &mut Connection, input: Input) -> Result<Value, String> {
    register_with_client(db, input, None)
}

pub fn register_with_client(
    db: &mut Connection,
    input: Input,
    client: Option<&str>,
) -> Result<Value, String> {
    validate_identity(&input)?;
    let transport = input.transport.as_deref().ok_or("缺少推送类型")?;
    let target = input.target.as_ref().ok_or("缺少推送目标")?;
    let destination = sender::validate_target(transport, target)?;
    let server = input.server.as_deref().ok_or("缺少 Core 地址")?;
    let url = reqwest::Url::parse(server).map_err(|_| "Core 地址无效")?;
    if !["https", "http"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || server.len() > 2048
    {
        return Err("Core 地址无效".into());
    }
    let locale = input.locale.as_deref().unwrap_or("zh-CN");
    if !["zh-CN", "en-US"].contains(&locale) {
        return Err("通知语言无效".into());
    }
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(error)?;
    super::check_client(&tx, client)?;
    let existing: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM push_devices WHERE id=?1)",
            [&input.id],
            |r| r.get(0),
        )
        .map_err(error)?;
    if existing {
        if !authorized(&tx, &input)? {
            return Err("设备凭据无效".into());
        }
        let baseline = crate::notifications::poll(
            &tx,
            crate::notifications::Request {
                after: None,
                instance: None,
            },
            None,
        )?;
        tx.execute("UPDATE push_devices SET transport=?2,target=?3,destination=?4,server=?5,locale=?6,revision=revision+1,
            cursor=CASE WHEN ?7 AND state='disabled' THEN ?8 ELSE cursor END,
            instance=CASE WHEN ?7 AND state='disabled' THEN ?9 ELSE instance END,
            state=CASE WHEN ?7 THEN 'active' ELSE state END,error=CASE WHEN ?7 THEN NULL ELSE error END,inflight=NULL WHERE id=?1",
            params![input.id,transport,target.to_string(),destination,server.trim_end_matches('/'),locale,input.activate,baseline["cursor"].as_i64(),baseline["instance"].as_str()]).map_err(|_| "推送目标已被其他设备绑定")?;
    } else {
        let page = crate::notifications::poll(
            &tx,
            crate::notifications::Request {
                after: None,
                instance: None,
            },
            None,
        )?;
        tx.execute("INSERT INTO push_devices(id,secret_hash,transport,target,destination,server,locale,cursor,instance) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![input.id,hash(&input.secret),transport,target.to_string(),destination,server.trim_end_matches('/'),locale,page["cursor"].as_i64(),page["instance"].as_str()]).map_err(|_| "推送目标已被其他设备绑定")?;
    }
    if let Some(client) = client {
        tx.execute("INSERT INTO client_auth_push_devices(device_id,client_id) VALUES(?1,?2) ON CONFLICT(device_id) DO UPDATE SET client_id=excluded.client_id", params![input.id,client]).map_err(error)?;
    }
    tx.commit().map_err(error)?;
    status(db, &input.id)
}
fn status(db: &Connection, id: &str) -> Result<Value, String> {
    db.query_row("SELECT transport,state,error FROM push_devices WHERE id=?1", [id], |r| Ok(json!({"registered":true,"transport":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"error":r.get::<_,Option<String>>(2)?})))
        .optional().map_err(error).map(|row| row.unwrap_or(json!({"registered":false,"state":"unregistered"})))
}
pub fn revoke(db: &Connection, id: &str) -> Result<(), String> {
    // Keep the identity so a token refresh cannot register a revoked device as new.
    db.execute("UPDATE push_devices SET state='disabled',error=NULL,inflight=NULL,revision=revision+1 WHERE id=?1", [id])
        .map(|_| ()).map_err(error)
}
pub fn device_api(db: &Connection, route: &str, input: Input) -> Result<Value, String> {
    validate_identity(&input)?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM push_devices WHERE id=?1)",
            [&input.id],
            |r| r.get(0),
        )
        .map_err(error)?;
    if !exists && ["/push/devices/status", "/push/devices/disable"].contains(&route) {
        return Ok(json!({"registered":false,"state":"unregistered"}));
    }
    if !authorized(db, &input)? {
        return Err("设备已解除绑定或设备凭据无效".into());
    }
    match route {
        "/push/devices/read" => {
            return super::reads::record(db, &input.id, input.read.ok_or("缺少已读快照")?)
        }
        "/push/devices/status" => (),
        "/push/devices/disable" => {
            revoke(db, &input.id)?;
        }
        "/push/devices/retry" => {
            db.execute("UPDATE push_devices SET state='active',error=NULL,inflight=NULL,revision=revision+1 WHERE id=?1 AND state='paused'", [&input.id]).map_err(error)?;
        }
        _ => return Err("推送接口不存在".into()),
    }
    status(db, &input.id)
}
pub fn list(db: &Connection) -> Result<Value, String> {
    let mut query = db
        .prepare("SELECT id,transport,state,error FROM push_devices ORDER BY id")
        .map_err(error)?;
    let rows = query.query_map([], |r| Ok(json!({"id":r.get::<_,String>(0)?,"transport":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"error":r.get::<_,Option<String>>(3)?}))).map_err(error)?;
    Ok(Value::Array(
        rows.collect::<Result<Vec<_>, _>>().map_err(error)?,
    ))
}

pub fn recover(db: &Connection) -> Result<(), String> {
    db.execute("UPDATE push_devices SET state='paused',error='delivery_unknown',inflight=NULL WHERE inflight IS NOT NULL", []).map(|_| ()).map_err(error)
}

pub struct Device {
    pub id: String,
    pub transport: String,
    pub target: Value,
    pub server: String,
    pub locale: String,
    cursor: i64,
    instance: String,
    revision: i64,
}
// Claim each event durably before contacting a provider. Revision guards prevent
// an old HTTP response overwriting a token refresh, revocation or manual retry.
pub fn dispatch(
    db: &mut Connection,
    slack: Option<&crate::plugin_connections::Connection>,
    mut send: impl FnMut(&Device, &Value) -> sender::Outcome,
) -> Result<(), String> {
    let devices = {
        let mut query = db.prepare("SELECT id,transport,target,server,locale,cursor,instance,revision FROM push_devices WHERE state='active' AND inflight IS NULL ORDER BY id").map_err(error)?;
        let rows = query
            .query_map([], |r| {
                Ok(Device {
                    id: r.get(0)?,
                    transport: r.get(1)?,
                    target: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or(Value::Null),
                    server: r.get(3)?,
                    locale: r.get(4)?,
                    cursor: r.get(5)?,
                    instance: r.get(6)?,
                    revision: r.get(7)?,
                })
            })
            .map_err(error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(error)?
    };
    for device in devices {
        let page = crate::notifications::poll(
            db,
            crate::notifications::Request {
                after: Some(device.cursor),
                instance: Some(device.instance.clone()),
            },
            slack,
        )?;
        let instance = page["instance"].as_str().ok_or("通知实例无效")?;
        let mut previous = device.cursor;
        let mut stopped = false;
        for item in page["items"].as_array().ok_or("通知列表无效")? {
            let id = item["id"].as_i64().ok_or("通知编号无效")?;
            if db.execute("UPDATE push_devices SET cursor=?3,inflight=?3,instance=?4 WHERE id=?1 AND revision=?2 AND state='active' AND inflight IS NULL",
                params![device.id,device.revision,id,instance]).map_err(error)? != 1 { stopped = true; break; }
            let payload = sender::payload(&device, instance, item);
            let outcome = if super::reads::contains(db, &device.id, instance, item)? {
                sender::Outcome::Delivered
            } else {
                send(&device, &payload)
            };
            let (state, failure, cursor) = match outcome {
                sender::Outcome::Delivered => ("active", None, id),
                sender::Outcome::Invalid => ("disabled", Some("subscription_expired"), id),
                sender::Outcome::Failed(reason, ambiguous) => (
                    "paused",
                    Some(reason),
                    if ambiguous { id } else { previous },
                ),
            };
            db.execute("UPDATE push_devices SET cursor=?3,state=?4,error=?5,inflight=NULL WHERE id=?1 AND revision=?2",
                params![device.id,device.revision,cursor,state,failure]).map_err(error)?;
            previous = id;
            if state != "active" {
                stopped = true;
                break;
            }
        }
        if !stopped {
            db.execute("UPDATE push_devices SET cursor=?3,instance=?4 WHERE id=?1 AND revision=?2 AND state='active' AND inflight IS NULL",
                params![device.id,device.revision,page["cursor"].as_i64(),instance]).map_err(error)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;

    fn setup() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        Store::open(&dir.path().join("state.db"))
            .unwrap()
            .append_message("chat", &json!({"role":"user","content":"private message"}))
            .unwrap();
        let db = super::super::open(&dir.path().join("state.db")).unwrap();
        (dir, db)
    }
    fn input(id: &str) -> Input {
        Input {
            id: id.into(),
            secret: URL_SAFE_NO_PAD.encode([42; 32]),
            transport: Some("android".into()),
            target: Some(json!("fake-private-fcm-token-123")),
            server: Some("https://core.example".into()),
            locale: Some("en-US".into()),
            activate: false,
            read: None,
        }
    }
    fn add(db: &Connection, n: i64, conversation: &str) {
        crate::notifications::insert(
            db,
            &format!("event-{n}"),
            "task_completed",
            "Task title",
            conversation,
        )
        .unwrap();
    }
    fn register_one(db: &mut Connection) {
        register(db, input("device-1234567890")).unwrap();
    }
    #[test]
    fn registration_needs_no_pairing_but_existing_devices_require_their_own_secret() {
        let (_dir, mut db) = setup();
        register_one(&mut db);
        // A second identity cannot take ownership of the same push target.
        assert!(register(&mut db, input("device-0987654321")).is_err());
        let mut next = input("device-0987654321");
        next.target = Some(json!("another-private-fcm-token-456"));
        assert_eq!(register(&mut db, next).unwrap()["state"], "active");
        let public = list(&db).unwrap().to_string();
        for private in ["private-fcm", &input("device-1234567890").secret] {
            assert!(!public.contains(private));
        }
        let mut wrong = input("device-1234567890");
        wrong.secret = URL_SAFE_NO_PAD.encode([1; 32]);
        let mut takeover = input("device-1234567890");
        takeover.secret = wrong.secret.clone();
        takeover.target = Some(json!("stolen-private-fcm-token-789"));
        assert!(register(&mut db, takeover).is_err());
        assert!(device_api(&db, "/push/devices/disable", wrong).is_err());
        assert_eq!(
            device_api(&db, "/push/devices/status", input("device-1234567890")).unwrap()["state"],
            "active"
        );
    }
    #[test]
    fn new_devices_skip_history_dispatch_once_and_filter_deleted_conversations() {
        let (_dir, mut db) = setup();
        add(&db, 1, "chat");
        register_one(&mut db);
        add(&db, 2, "deleted");
        add(&db, 3, "chat");
        let mut delivered = vec![];
        dispatch(&mut db, None, |_, payload| {
            delivered.push(payload.clone());
            sender::Outcome::Delivered
        })
        .unwrap();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0]["id"], "3");
        assert_eq!(delivered[0]["path"], "/chat/chat");
        assert!(!delivered[0].to_string().contains("private"));
        dispatch(&mut db, None, |_, _| panic!("duplicate delivery")).unwrap();
        assert_eq!(
            db.query_row("SELECT cursor FROM push_devices", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            3
        );
    }
    #[test]
    fn definite_failures_pause_and_retry_but_unknown_outcomes_never_replay() {
        let (_dir, mut db) = setup();
        register_one(&mut db);
        add(&db, 1, "chat");
        dispatch(&mut db, None, |_, _| {
            sender::Outcome::Failed("provider_rejected", false)
        })
        .unwrap();
        assert_eq!(status(&db, "device-1234567890").unwrap()["state"], "paused");
        dispatch(&mut db, None, |_, _| panic!("automatic retry")).unwrap();
        device_api(&db, "/push/devices/retry", input("device-1234567890")).unwrap();
        dispatch(&mut db, None, |_, _| {
            sender::Outcome::Failed("delivery_unknown", true)
        })
        .unwrap();
        device_api(&db, "/push/devices/retry", input("device-1234567890")).unwrap();
        dispatch(&mut db, None, |_, _| panic!("unknown event replayed")).unwrap();
        add(&db, 2, "chat");
        dispatch(&mut db, None, |_, _| sender::Outcome::Invalid).unwrap();
        assert_eq!(
            status(&db, "device-1234567890").unwrap()["state"],
            "disabled"
        );
        assert_eq!(
            device_api(&db, "/push/devices/retry", input("device-1234567890")).unwrap()["state"],
            "disabled"
        );
    }
    #[test]
    fn crashed_claims_pause_and_stale_provider_responses_cannot_reactivate_revoked_devices() {
        let (dir, mut db) = setup();
        register_one(&mut db);
        add(&db, 1, "chat");
        db.execute("UPDATE push_devices SET cursor=1,inflight=1", [])
            .unwrap();
        recover(&db).unwrap();
        assert_eq!(
            status(&db, "device-1234567890").unwrap()["error"],
            "delivery_unknown"
        );
        device_api(&db, "/push/devices/retry", input("device-1234567890")).unwrap();
        add(&db, 2, "chat");
        dispatch(&mut db, None, |_, _| {
            let other = Connection::open(dir.path().join("state.db")).unwrap();
            device_api(&other, "/push/devices/disable", input("device-1234567890")).unwrap();
            sender::Outcome::Delivered
        })
        .unwrap();
        assert_eq!(
            status(&db, "device-1234567890").unwrap()["state"],
            "disabled"
        );
    }
    #[test]
    fn token_refresh_does_not_resume_failed_deliveries() {
        let (_dir, mut db) = setup();
        register_one(&mut db);
        add(&db, 1, "chat");
        dispatch(&mut db, None, |_, _| {
            sender::Outcome::Failed("provider_rejected", false)
        })
        .unwrap();
        let mut next = input("device-1234567890");
        next.target = Some(json!("new-private-fcm-token-123"));
        assert_eq!(register(&mut db, next).unwrap()["state"], "paused");
        dispatch(&mut db, None, |_, _| panic!("refresh resumed delivery")).unwrap();
    }
    #[test]
    fn revocation_survives_automatic_registration_and_requires_explicit_activation() {
        let (_dir, mut db) = setup();
        register_one(&mut db);
        revoke(&db, "device-1234567890").unwrap();
        add(&db, 1, "chat");
        assert_eq!(
            register(&mut db, input("device-1234567890")).unwrap()["state"],
            "disabled"
        );
        dispatch(&mut db, None, |_, _| panic!("revoked device reactivated")).unwrap();
        let mut next = input("device-1234567890");
        next.activate = true;
        assert_eq!(register(&mut db, next).unwrap()["state"], "active");
        dispatch(&mut db, None, |_, _| panic!("revoked history replayed")).unwrap();
    }
    #[test]
    fn re_enabling_remote_skips_events_already_seen_locally_while_disabled() {
        let (_dir, mut db) = setup();
        register_one(&mut db);
        device_api(&db, "/push/devices/disable", input("device-1234567890")).unwrap();
        add(&db, 1, "chat");
        let mut next = input("device-1234567890");
        next.activate = true;
        register(&mut db, next).unwrap();
        dispatch(&mut db, None, |_, _| panic!("disabled history replayed")).unwrap();
        add(&db, 2, "chat");
        let mut count = 0;
        dispatch(&mut db, None, |_, _| {
            count += 1;
            sender::Outcome::Delivered
        })
        .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn read_receipts_only_suppress_the_same_device_scope_and_snapshot() {
        let (dir, mut db) = setup();
        Store::open(&dir.path().join("state.db"))
            .unwrap()
            .append_message("other", &json!({"role":"user","content":"other"}))
            .unwrap();
        register_one(&mut db);
        let mut second = input("device-0987654321");
        second.target = Some(json!("another-private-fcm-token-456"));
        register(&mut db, second).unwrap();
        add(&db, 1, "chat");
        add(&db, 2, "other");
        let snapshot =
            crate::notifications::snapshot(&db, json!(["chat", "chat"]).to_string()).unwrap();
        add(&db, 3, "chat");
        let mut read = input("device-1234567890");
        read.read = Some(serde_json::from_value(snapshot.clone()).unwrap());
        device_api(&db, "/push/devices/read", read).unwrap();
        // Repeated or out-of-order receipts never move a scope backwards.
        let mut earlier = snapshot.clone();
        earlier["cursor"] = json!(0);
        let mut read = input("device-1234567890");
        read.read = Some(serde_json::from_value(earlier).unwrap());
        device_api(&db, "/push/devices/read", read).unwrap();
        let mut delivered = vec![];
        dispatch(&mut db, None, |device, payload| {
            delivered.push((
                device.id.clone(),
                payload["id"].as_str().unwrap().to_string(),
            ));
            sender::Outcome::Delivered
        })
        .unwrap();
        assert!(!delivered.contains(&("device-1234567890".into(), "1".into())));
        for pair in [
            ("device-1234567890", "2"),
            ("device-1234567890", "3"),
            ("device-0987654321", "1"),
            ("device-0987654321", "2"),
            ("device-0987654321", "3"),
        ] {
            assert!(delivered.contains(&(pair.0.into(), pair.1.into())));
        }
        dispatch(&mut db, None, |_, _| panic!("read event replayed")).unwrap();
    }

    #[test]
    fn read_receipts_require_device_credentials_and_cannot_acknowledge_future_events() {
        let (_dir, mut db) = setup();
        register_one(&mut db);
        db.execute(
            "INSERT INTO approvals(task_id,action,payload,status) VALUES(0,'test','{}','pending')",
            [],
        )
        .unwrap();
        add(&db, 1, "chat");
        let snapshot =
            crate::notifications::snapshot(&db, json!(["chat", "chat"]).to_string()).unwrap();
        let mut wrong = input("device-1234567890");
        wrong.secret = URL_SAFE_NO_PAD.encode([1; 32]);
        wrong.read = Some(serde_json::from_value(snapshot.clone()).unwrap());
        assert!(device_api(&db, "/push/devices/read", wrong).is_err());
        for invalid in ["future", "instance", "scope"] {
            let mut value = snapshot.clone();
            match invalid {
                "future" => value["cursor"] = json!(2),
                "instance" => value["instance"] = json!("replaced-db"),
                _ => value["scope"] = json!("[]"),
            };
            let mut read = input("device-1234567890");
            read.read = Some(serde_json::from_value(value).unwrap());
            assert!(device_api(&db, "/push/devices/read", read).is_err());
        }
        let mut read = input("device-1234567890");
        read.read = Some(serde_json::from_value(snapshot).unwrap());
        device_api(&db, "/push/devices/read", read).unwrap();
        assert_eq!(
            db.query_row("SELECT status FROM approvals", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "pending"
        );
    }
}
