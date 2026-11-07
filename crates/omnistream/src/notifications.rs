//! A durable cursor feed shared by desktop and browser notification clients.
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn snapshot(db: &Connection, scope: String) -> Result<Value, String> {
    db.query_row(
        "SELECT id,(SELECT COALESCE(MAX(id),0) FROM notifications) FROM notification_instance LIMIT 1",
        [], |r| Ok(json!({"instance":r.get::<_,String>(0)?,"cursor":r.get::<_,i64>(1)?,"scope":scope})),
    ).map_err(|e| e.to_string())
}

pub fn scope(item: &Value) -> String {
    if item["channel"].is_string() {
        json!(["slack", item["team"], item["owner"], item["channel"]]).to_string()
    } else {
        json!(["chat", item["conversation"]]).to_string()
    }
}

pub fn schema(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS notifications (
            id INTEGER PRIMARY KEY AUTOINCREMENT, event_key TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL, title TEXT NOT NULL, conversation TEXT,
            approval_id INTEGER, team TEXT, owner TEXT, channel TEXT);
         CREATE TABLE IF NOT EXISTS notification_instance (id TEXT NOT NULL);
         INSERT INTO notification_instance(id) SELECT lower(hex(randomblob(16)))
            WHERE NOT EXISTS(SELECT 1 FROM notification_instance);",
    )
    .map_err(|e| e.to_string())
}

pub fn insert(
    db: &Connection,
    key: &str,
    kind: &str,
    title: &str,
    conversation: &str,
) -> Result<(), String> {
    db.execute(
        "INSERT OR IGNORE INTO notifications(event_key,kind,title,conversation) VALUES (?1,?2,?3,?4)",
        params![key, kind, title.chars().take(160).collect::<String>(), conversation],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub after: Option<i64>,
    pub instance: Option<String>,
}

pub fn poll(
    db: &Connection,
    input: Request,
    slack: Option<&crate::plugin_connections::Connection>,
) -> Result<Value, String> {
    if input.after.is_some_and(|id| id < 0) {
        return Err("通知游标无效".into());
    }
    // Starting a client never replays old task results or an existing inbox.
    let latest: i64 = db
        .query_row("SELECT COALESCE(MAX(id),0) FROM notifications", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let instance: String = db
        .query_row("SELECT id FROM notification_instance LIMIT 1", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let Some(after) = input
        .after
        .filter(|id| *id <= latest && input.instance.as_ref() == Some(&instance))
    else {
        return Ok(json!({"items":[],"cursor":latest,"instance":instance}));
    };
    let mut query = db.prepare(
        "SELECT n.id,n.kind,n.title,n.conversation,n.channel,n.team,n.owner,
            CASE WHEN n.approval_id IS NOT NULL THEN EXISTS(SELECT 1 FROM approvals a WHERE a.id=n.approval_id AND a.status='pending')
                WHEN n.kind='approval_pending' THEN EXISTS(SELECT 1 FROM app_settings a,json_each(a.data,'$.items') i,json_each(i.value,'$.confirmations') c
                    WHERE a.key='automations' AND json_extract(c.value,'$.conversation')=n.conversation AND json_extract(c.value,'$.status')='pending')
                ELSE 1 END,
            COALESCE(json_extract(s.data,'$.archived'),0),
            CASE WHEN n.conversation IS NULL THEN 1 ELSE EXISTS(SELECT 1 FROM conversation_activity c WHERE c.id=n.conversation) END
         FROM notifications n LEFT JOIN conversation_settings s ON s.id=n.conversation
         WHERE n.id>?1 ORDER BY n.id LIMIT 50",
    ).map_err(|e| e.to_string())?;
    let rows = query.query_map([after], |r| {
        Ok((json!({"id":r.get::<_,i64>(0)?,"kind":r.get::<_,String>(1)?,"title":r.get::<_,String>(2)?,"conversation":r.get::<_,Option<String>>(3)?,"channel":r.get::<_,Option<String>>(4)?}),
            r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?,
            r.get::<_,bool>(7)?,r.get::<_,bool>(8)?,r.get::<_,bool>(9)?))
    }).map_err(|e| e.to_string())?;
    let mut cursor = after;
    let mut items = vec![];
    for row in rows {
        let (mut item, team, owner, pending, archived, exists) = row.map_err(|e| e.to_string())?;
        cursor = item["id"].as_i64().unwrap();
        let current_account = team.is_none()
            || slack.is_some_and(|c| {
                c.enabled
                    && c.listen
                    && c.team_id == team
                    && c.user_id == owner
                    && c.auth_mode == crate::plugin_connections::AuthMode::SlackUser
            });
        if pending && !archived && exists && current_account && item["kind"] != "dismissed" {
            item["team"] = json!(team);
            item["owner"] = json!(owner);
            item["scope"] = json!(scope(&item));
            items.push(item);
        }
    }
    Ok(json!({"items":items,"cursor":cursor,"instance":instance}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{storage::Store, AgentExecution};

    fn request(page: &Value) -> Request {
        Request {
            after: page["cursor"].as_i64(),
            instance: page["instance"].as_str().map(str::to_string),
        }
    }
    fn baseline(store: &Store) -> Value {
        store
            .notifications(
                Request {
                    after: None,
                    instance: None,
                },
                None,
            )
            .unwrap()
    }
    fn execution(id: u64, status: &str) -> AgentExecution {
        serde_json::from_value(json!({"id":id,"transient":true,"project":"/tmp/project","title":"check project","goal":"private goal","status":status,"last_output":"private output"})).unwrap()
    }

    #[test]
    fn notifications_follow_terminal_transitions_once_and_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let mut store = Store::open(&path).unwrap();
        store
            .append_message("chat", &json!({"role":"user","content":"hello"}))
            .unwrap();
        let start = baseline(&store);
        for (id, status) in [(1, "unverified"), (2, "failed"), (3, "cancelled")] {
            store.save(&execution(id, "running")).unwrap();
            store.save_call(id, "chat", "scope", "agent").unwrap();
            store.save(&execution(id, status)).unwrap();
            store.save(&execution(id, status)).unwrap();
        }
        drop(store);
        let store = Store::open(&path).unwrap();
        let page = store.notifications(request(&start), None).unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
        assert_eq!(page["items"][0]["kind"], "task_completed");
        assert_eq!(page["items"][1]["kind"], "task_failed");
        assert!(!page.to_string().contains("private"));
        assert!(baseline(&store)["items"].as_array().unwrap().is_empty());
        assert!(store.notifications(request(&page), None).unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn notifications_filter_resolved_approvals_archived_and_deleted_conversations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let store = Store::open(&path).unwrap();
        store
            .append_message("chat", &json!({"role":"user","content":"hello"}))
            .unwrap();
        let start = baseline(&store);
        let payload = json!({"conversation":"chat","secret":"never expose"});
        let approved = store
            .create_approval(None, "plugin.invoke", &payload)
            .unwrap();
        store.resolve_approval(approved, "approved").unwrap();
        store
            .create_approval(None, "plugin.invoke", &payload)
            .unwrap();
        let page = store.notifications(request(&start), None).unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert!(!page.to_string().contains("secret"));
        store.set_conversation_archived("chat", true).unwrap();
        assert_eq!(
            store.notifications(request(&start), None).unwrap()["items"],
            json!([])
        );
        store.delete_conversation("chat").unwrap();
        assert_eq!(
            store.notifications(request(&start), None).unwrap()["items"],
            json!([])
        );
    }

    #[test]
    fn notifications_paginate_filtered_rows_and_rebase_replaced_databases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let store = Store::open(&path).unwrap();
        let start = baseline(&store);
        let db = Connection::open(&path).unwrap();
        for id in 0..120 {
            insert(
                &db,
                &format!("missing:{id}"),
                "task_completed",
                "test",
                "deleted-chat",
            )
            .unwrap();
        }
        let first = store.notifications(request(&start), None).unwrap();
        assert_eq!(first["cursor"], 50);
        assert_eq!(first["items"], json!([]));
        let second = store.notifications(request(&first), None).unwrap();
        assert_eq!(second["cursor"], 100);
        assert_eq!(
            store.notifications(request(&second), None).unwrap()["cursor"],
            120
        );
        let reset = store
            .notifications(
                Request {
                    after: Some(1),
                    instance: Some("another-database".into()),
                },
                None,
            )
            .unwrap();
        assert_eq!(reset["cursor"], 120);
        assert_eq!(reset["items"], json!([]));
        assert!(store
            .notifications(
                Request {
                    after: Some(-1),
                    instance: None
                },
                None
            )
            .is_err());
    }

    #[test]
    fn notifications_inbox_events_are_new_incoming_account_scoped_and_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let store = Store::open(&path).unwrap();
        let inbox = crate::slack_inbox::Inbox::open(&path).unwrap();
        let mut connection: crate::plugin_connections::Connection = serde_json::from_value(json!({"token":"private-token","auth_mode":"slack_user","listen":true,"team_id":"T1","user_id":"U1","enabled":true,"allow_writes":false})).unwrap();
        let start = baseline(&store);
        let incoming = json!({"type":"event_callback","team_id":"T1","authorizations":[{"user_id":"U1","is_bot":false}],"event_id":"first","event":{"type":"message","channel":"D1","channel_type":"im","user":"U2","ts":"100.001","text":"private message"}});
        inbox.ingest(&connection, &incoming).unwrap();
        inbox.ingest(&connection, &incoming).unwrap();
        inbox
            .sent(
                &connection,
                &json!({"channel":"D1","text":"sent"}),
                &json!({"data":{"ts":"100.002"}}),
            )
            .unwrap();
        let page = store
            .notifications(request(&start), Some(&connection))
            .unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["items"][0]["channel"], "D1");
        assert!(!page.to_string().contains("private"));
        connection.user_id = Some("other-user".into());
        assert_eq!(
            store
                .notifications(request(&start), Some(&connection))
                .unwrap()["items"],
            json!([])
        );
        connection.user_id = Some("U1".into());
        let mut deletion = incoming;
        deletion["event_id"] = json!("delete");
        deletion["event"] = json!({"type":"message","subtype":"message_deleted","channel":"D1","channel_type":"im","event_ts":"101.001","deleted_ts":"100.001","previous_message":{"user":"U2","ts":"100.001"}});
        inbox.ingest(&connection, &deletion).unwrap();
        assert_eq!(
            store
                .notifications(request(&start), Some(&connection))
                .unwrap()["items"],
            json!([])
        );
    }
}
