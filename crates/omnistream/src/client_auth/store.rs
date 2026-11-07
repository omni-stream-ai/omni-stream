use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const REQUEST_TTL: i64 = 600;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub fn random(bytes: usize) -> Result<String, String> {
    let mut value = vec![0u8; bytes];
    getrandom::fill(&mut value).map_err(|_| "无法生成客户端授权凭据")?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

#[derive(Clone, Debug, Serialize)]
pub struct Record {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub address: String,
    pub origin: String,
    pub code: String,
    pub status: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub approved_at: Option<i64>,
    pub approved_by: Option<String>,
}

pub struct Store(pub Connection);

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        db.busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let exists: bool = db.query_row("SELECT COUNT(*)=2 FROM sqlite_master WHERE type='table' AND name IN ('client_authorizations','client_auth_push_devices')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if !exists {
            db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS client_authorizations (
                id TEXT PRIMARY KEY, token_hash TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL, platform TEXT NOT NULL, address TEXT NOT NULL, origin TEXT NOT NULL,
                code TEXT NOT NULL, status TEXT NOT NULL,
                created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
                approved_at INTEGER, approved_by TEXT
            );
            CREATE INDEX IF NOT EXISTS client_authorizations_status ON client_authorizations(status,created_at);
            CREATE TABLE IF NOT EXISTS client_auth_push_devices (
                device_id TEXT PRIMARY KEY, client_id TEXT NOT NULL
            ); COMMIT;").map_err(|e| e.to_string())?;
        }
        Ok(Self(db))
    }

    pub fn authorized(&self, token: &str) -> Result<Option<String>, String> {
        self.0
            .query_row(
                "SELECT id FROM client_authorizations WHERE token_hash=?1 AND status='approved'",
                [hash(token)],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn active(&self, id: &str) -> Result<bool, String> {
        self.0.query_row("SELECT EXISTS(SELECT 1 FROM client_authorizations WHERE id=?1 AND status='approved')", [id], |r|r.get(0)).map_err(|e|e.to_string())
    }

    fn expire(&self) -> Result<(), String> {
        self.0.execute("UPDATE client_authorizations SET status='expired' WHERE status='pending' AND expires_at<=?1", [now()]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Record>, String> {
        self.expire()?;
        self.0.query_row("SELECT id,name,platform,address,origin,code,status,created_at,expires_at,approved_at,approved_by FROM client_authorizations WHERE id=?1", [id], read).optional().map_err(|e| e.to_string())
    }

    pub fn own(&self, id: &str, token: &str) -> Result<Option<Record>, String> {
        let exists: bool = self
            .0
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM client_authorizations WHERE id=?1 AND token_hash=?2)",
                params![id, hash(token)],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            self.get(id)
        } else {
            Ok(None)
        }
    }

    pub fn list(&self, pending: bool) -> Result<Vec<Record>, String> {
        self.expire()?;
        let mut query = self.0.prepare("SELECT id,name,platform,address,origin,code,status,created_at,expires_at,approved_at,approved_by FROM client_authorizations WHERE (?1=1 AND status='pending') OR (?1=0 AND status IN ('approved','revoked')) ORDER BY created_at DESC,id LIMIT 256").map_err(|e| e.to_string())?;
        let rows = query
            .query_map([pending], read)
            .map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }

    pub fn request(
        &mut self,
        name: &str,
        platform: &str,
        address: &str,
        origin: &str,
        token: &str,
    ) -> Result<Record, String> {
        if name.trim().is_empty()
            || name.chars().count() > 80
            || platform.chars().count() > 80
            || name.chars().chain(platform.chars()).any(char::is_control)
            || origin.len() > 1024
            || URL_SAFE_NO_PAD
                .decode(token)
                .map_or(true, |v| v.len() != 32)
        {
            return Err("授权申请格式无效".into());
        }
        self.expire()?;
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM client_authorizations WHERE token_hash=?1",
                [hash(token)],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(id) = existing {
            tx.commit().map_err(|e| e.to_string())?;
            return self.get(&id)?.ok_or("授权申请不存在".into());
        }
        let time = now();
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM client_authorizations WHERE address=?1 AND created_at>?2",
                params![address, time - REQUEST_TTL],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let pending: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM client_authorizations WHERE status='pending'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if count >= 5 || pending >= 64 {
            return Err("授权申请过于频繁，请稍后重试".into());
        }
        tx.execute("DELETE FROM client_authorizations WHERE status IN ('expired','rejected','cancelled') AND created_at<?1", [time-86400]).map_err(|e|e.to_string())?;
        let id = random(16)?;
        let mut code_bytes = [0u8; 3];
        getrandom::fill(&mut code_bytes).map_err(|_| "无法生成授权核对码")?;
        let code = code_bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        tx.execute("INSERT INTO client_authorizations(id,token_hash,name,platform,address,origin,code,status,created_at,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',?8,?9)", params![id,hash(token),name.trim(),platform,address,origin,code,time,time+REQUEST_TTL]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.get(&id)?.ok_or("授权申请不存在".into())
    }

    pub fn decide(
        &mut self,
        id: &str,
        code: &str,
        approve: bool,
        actor: &str,
    ) -> Result<Record, String> {
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if actor!="local" && !tx.query_row("SELECT EXISTS(SELECT 1 FROM client_authorizations WHERE id=?1 AND status='approved')", [actor], |r|r.get::<_,bool>(0)).map_err(|e|e.to_string())? {
            return Err("当前客户端已被撤销授权".into());
        }
        let status = if approve { "approved" } else { "rejected" };
        let count = tx.execute("UPDATE client_authorizations SET status=?3,approved_at=?4,approved_by=?5 WHERE id=?1 AND code=?2 AND status='pending' AND expires_at>?4", params![id,code,status,now(),actor]).map_err(|e|e.to_string())?;
        if count != 1 {
            return Err("授权申请已处理、已过期或核对码不匹配".into());
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.get(id)?.ok_or("授权申请不存在".into())
    }

    pub fn cancel(&self, id: &str, token: &str) -> Result<Record, String> {
        self.0.execute("UPDATE client_authorizations SET status='cancelled' WHERE id=?1 AND token_hash=?2 AND status='pending'", params![id,hash(token)]).map_err(|e|e.to_string())?;
        self.own(id, token)?.ok_or("授权申请不存在".into())
    }

    pub fn revoke(&mut self, id: &str, actor: &str) -> Result<(), String> {
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if actor!="local" && !tx.query_row("SELECT EXISTS(SELECT 1 FROM client_authorizations WHERE id=?1 AND status='approved')", [actor], |r|r.get::<_,bool>(0)).map_err(|e|e.to_string())? { return Err("当前客户端已被撤销授权".into()); }
        if tx.execute("UPDATE client_authorizations SET status='revoked' WHERE id=?1 AND status='approved'", [id]).map_err(|e|e.to_string())?!=1 { return Err("客户端不存在或已撤销".into()); }
        let push: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='push_devices')", [], |r|r.get(0)).map_err(|e|e.to_string())?;
        if push {
            tx.execute("UPDATE push_devices SET state='disabled',error=NULL,inflight=NULL,revision=revision+1 WHERE id IN (SELECT device_id FROM client_auth_push_devices WHERE client_id=?1)", [id]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
}

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    Ok(Record {
        id: row.get(0)?,
        name: row.get(1)?,
        platform: row.get(2)?,
        address: row.get(3)?,
        origin: row.get(4)?,
        code: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
        expires_at: row.get(8)?,
        approved_at: row.get(9)?,
        approved_by: row.get(10)?,
    })
}
