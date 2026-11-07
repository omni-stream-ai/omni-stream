use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    instance: String,
    scope: String,
    cursor: i64,
}

pub fn schema(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS push_reads(
        device TEXT NOT NULL,instance TEXT NOT NULL,scope TEXT NOT NULL,cursor INTEGER NOT NULL,
        PRIMARY KEY(device,instance,scope));",
    )
    .map_err(|e| e.to_string())
}

pub fn record(db: &Connection, device: &str, read: Receipt) -> Result<Value, String> {
    let scope: Vec<String> = serde_json::from_str(&read.scope).map_err(|_| "通知范围无效")?;
    if !matches!(scope.first().map(String::as_str), Some("chat") if scope.len()==2)
        && !matches!(scope.first().map(String::as_str), Some("slack") if scope.len()==4)
    {
        return Err("通知范围无效".into());
    }
    if scope
        .iter()
        .any(|part| part.is_empty() || part.len() > 2048)
    {
        return Err("通知范围无效".into());
    }
    let current = crate::notifications::snapshot(db, read.scope.clone())?;
    if read.instance != current["instance"].as_str().unwrap_or_default()
        || read.cursor < 0
        || read.cursor > current["cursor"].as_i64().unwrap_or_default()
    {
        return Err("已读通知快照已失效，请刷新内容".into());
    }
    let scope = serde_json::to_string(&scope).map_err(|_| "通知范围无效")?;
    db.execute(
        "INSERT INTO push_reads(device,instance,scope,cursor) VALUES(?1,?2,?3,?4)
        ON CONFLICT(device,instance,scope) DO UPDATE SET cursor=MAX(cursor,excluded.cursor)",
        params![device, read.instance, scope, read.cursor],
    )
    .map_err(|e| e.to_string())?;
    Ok(json!({"read":true}))
}

pub fn contains(
    db: &Connection,
    device: &str,
    instance: &str,
    item: &Value,
) -> Result<bool, String> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM push_reads WHERE device=?1 AND instance=?2 AND scope=?3 AND cursor>=?4)",
        params![device,instance,crate::notifications::scope(item),item["id"].as_i64()], |row| row.get(0))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_receipts_are_scoped_to_workspace_account_and_channel() {
        let db = Connection::open_in_memory().unwrap();
        crate::notifications::schema(&db).unwrap();
        schema(&db).unwrap();
        crate::notifications::insert(&db, "one", "inbox_message", "D1", "chat").unwrap();
        let item = json!({"id":1,"team":"T1","owner":"U1","channel":"D1"});
        let snapshot =
            crate::notifications::snapshot(&db, crate::notifications::scope(&item)).unwrap();
        record(
            &db,
            "device",
            serde_json::from_value(snapshot.clone()).unwrap(),
        )
        .unwrap();
        let instance = snapshot["instance"].as_str().unwrap();
        assert!(contains(&db, "device", instance, &item).unwrap());
        for field in ["team", "owner", "channel"] {
            let mut another = item.clone();
            another[field] = json!("other");
            assert!(!contains(&db, "device", instance, &another).unwrap());
        }
        assert!(!contains(&db, "another-device", instance, &item).unwrap());
        assert!(!contains(&db, "device", "another-instance", &item).unwrap());
    }
}
