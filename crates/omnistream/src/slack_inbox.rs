//! Durable, account-scoped Slack messages. Socket acknowledgements follow commits.
use crate::plugin_connections::{Connection, ListenScope};
use rusqlite::params;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

pub struct Inbox(rusqlite::Connection);
impl Inbox {
    pub fn open(path: &Path) -> Result<Self, String> {
        let db = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS slack_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL, owner TEXT NOT NULL,
            channel TEXT NOT NULL, channel_type TEXT NOT NULL, ts TEXT NOT NULL,
            sender TEXT NOT NULL, text TEXT NOT NULL, thread_ts TEXT, version TEXT NOT NULL,
            deleted INTEGER NOT NULL DEFAULT 0, edited INTEGER NOT NULL DEFAULT 0,
            UNIQUE(team,owner,channel,ts));
            CREATE TABLE IF NOT EXISTS slack_event_ids (team TEXT NOT NULL, owner TEXT NOT NULL, event_id TEXT NOT NULL, PRIMARY KEY(team,owner,event_id));
            CREATE TABLE IF NOT EXISTS slack_threads (team TEXT NOT NULL, owner TEXT NOT NULL, channel TEXT NOT NULL, ts TEXT NOT NULL, PRIMARY KEY(team,owner,channel,ts));")
            .map_err(|e| e.to_string())?;
        Ok(Self(db))
    }

    pub fn ingest(&self, connection: &Connection, payload: &Value) -> Result<bool, String> {
        let (team, owner) = identity(connection)?;
        if payload["type"] != "event_callback" || payload["team_id"] != team {
            return Ok(false);
        }
        // A Socket Mode app can receive events for multiple installations. Never
        // import another user's or a bot's authorization into this user's inbox.
        let authorized = payload["authorizations"].as_array().is_some_and(|items| {
            items.iter().any(|item| {
                item["user_id"] == owner
                    && item["is_bot"] == false
                    && item["team_id"].as_str().is_none_or(|id| id == team)
            })
        }) || payload["authed_users"]
            .as_array()
            .is_some_and(|items| items.iter().any(|id| id == owner));
        if !authorized {
            return Ok(false);
        }
        self.message(
            connection,
            &payload["event"],
            payload["event_id"].as_str(),
            false,
        )
    }

    pub fn sent(
        &self,
        connection: &Connection,
        input: &Value,
        result: &Value,
    ) -> Result<(), String> {
        let ts = result["data"]["ts"]
            .as_str()
            .ok_or("Slack 未返回已发送消息的时间戳")?;
        self.message(
            connection,
            &json!({"type":"message","channel":input["channel"],"user":connection.user_id,
            "text":input["text"],"ts":ts,"thread_ts":input["thread_ts"]}),
            None,
            true,
        )?;
        Ok(())
    }

    fn message(
        &self,
        connection: &Connection,
        event: &Value,
        event_id: Option<&str>,
        sent: bool,
    ) -> Result<bool, String> {
        let (team, owner) = identity(connection)?;
        if event["type"] != "message" {
            return Ok(false);
        }
        let Some(channel) = event["channel"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 200)
        else {
            return Ok(false);
        };
        let subtype = event["subtype"].as_str().unwrap_or_default();
        if ![
            "",
            "bot_message",
            "file_share",
            "thread_broadcast",
            "reply_broadcast",
            "me_message",
            "message_changed",
            "message_deleted",
        ]
        .contains(&subtype)
        {
            return Ok(false);
        }
        let deleted = subtype == "message_deleted";
        let edited = subtype == "message_changed";
        let message = if edited {
            &event["message"]
        } else if deleted {
            &event["previous_message"]
        } else {
            event
        };
        let Some(ts) = if deleted {
            event["deleted_ts"].as_str()
        } else {
            message["ts"].as_str()
        }
        .filter(|ts| valid_ts(ts)) else {
            return Ok(false);
        };
        let sender = message["user"]
            .as_str()
            .or_else(|| message["bot_id"].as_str())
            .unwrap_or("");
        let mut text = message["text"]
            .as_str()
            .unwrap_or_default()
            .replace(&connection.token, "[redacted]");
        if let Some(token) = &connection.app_token {
            text = text.replace(token, "[redacted]");
        }
        let thread = message["thread_ts"].as_str().filter(|ts| valid_ts(ts));
        let root = thread.unwrap_or(ts);
        let channel_type = event["channel_type"]
            .as_str()
            .unwrap_or(if channel.starts_with('D') {
                "im"
            } else {
                "channel"
            });
        let tx = self.0.unchecked_transaction().map_err(|e| e.to_string())?;
        let existing: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM slack_messages WHERE team=?1 AND owner=?2 AND channel=?3 AND ts=?4)", params![team,owner,channel,ts], |row| row.get(0)).map_err(|e| e.to_string())?;
        let watched: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM slack_threads WHERE team=?1 AND owner=?2 AND channel=?3 AND ts=?4)", params![team,owner,channel,root], |row| row.get(0)).map_err(|e| e.to_string())?;
        let related = matches!(channel_type, "im" | "mpim")
            || sender == owner
            || text.contains(&format!("<@{owner}>"))
            || watched;
        if !sent
            && !existing
            && (deleted || (connection.listen_scope == ListenScope::Related && !related))
        {
            return Ok(false);
        }
        if let Some(event_id) = event_id {
            if tx
                .execute(
                    "INSERT OR IGNORE INTO slack_event_ids(team,owner,event_id) VALUES (?1,?2,?3)",
                    params![team, owner, event_id],
                )
                .map_err(|e| e.to_string())?
                == 0
            {
                return Ok(false);
            }
        }
        let version = event["event_ts"]
            .as_str()
            .or_else(|| message.pointer("/edited/ts").and_then(Value::as_str))
            .filter(|ts| valid_ts(ts))
            .unwrap_or(ts);
        tx.execute("INSERT INTO slack_messages(team,owner,channel,channel_type,ts,sender,text,thread_ts,version,deleted,edited)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
            ON CONFLICT(team,owner,channel,ts) DO UPDATE SET channel_type=excluded.channel_type,sender=excluded.sender,text=excluded.text,
            thread_ts=excluded.thread_ts,version=excluded.version,deleted=excluded.deleted,edited=excluded.edited
            WHERE CAST(excluded.version AS REAL)>=CAST(slack_messages.version AS REAL)",
            params![team,owner,channel,channel_type,ts,sender,if deleted { "" } else { &text },thread,version,deleted,edited]).map_err(|e| e.to_string())?;
        if related || sent {
            tx.execute(
                "INSERT OR IGNORE INTO slack_threads(team,owner,channel,ts) VALUES (?1,?2,?3,?4)",
                params![team, owner, channel, root],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub fn conversations(&self, connection: &Connection) -> Result<Value, String> {
        let (team, owner) = identity(connection)?;
        let mut query = self.0.prepare("SELECT channel,channel_type,ts,substr(text,1,200),total FROM (
            SELECT *,COUNT(*) OVER(PARTITION BY channel) AS total,ROW_NUMBER() OVER(PARTITION BY channel ORDER BY CAST(ts AS REAL) DESC,id DESC) AS position
            FROM slack_messages WHERE team=?1 AND owner=?2) WHERE position=1 ORDER BY CAST(ts AS REAL) DESC").map_err(|e| e.to_string())?;
        let rows = query.query_map(params![team,owner], |row| Ok(json!({"channel":row.get::<_,String>(0)?,"channel_type":row.get::<_,String>(1)?,"ts":row.get::<_,String>(2)?,"text":row.get::<_,String>(3)?,"count":row.get::<_,u64>(4)?}))).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
            .map_err(|e| e.to_string())
    }

    pub fn messages(
        &self,
        connection: &Connection,
        channel: &str,
        before: Option<&str>,
        limit: u64,
    ) -> Result<Value, String> {
        let (team, owner) = identity(connection)?;
        if !(1..=100).contains(&limit)
            || before.is_some_and(|ts| !valid_ts(ts))
            || channel.is_empty()
            || channel.len() > 200
        {
            return Err("收件箱分页参数无效".into());
        }
        let mut query = self.0.prepare("SELECT id,ts,sender,text,thread_ts,deleted,edited FROM slack_messages
            WHERE team=?1 AND owner=?2 AND channel=?3 AND (?4 IS NULL OR CAST(ts AS REAL)<CAST(?4 AS REAL)) ORDER BY CAST(ts AS REAL) DESC,id DESC LIMIT ?5").map_err(|e| e.to_string())?;
        let rows = query.query_map(params![team,owner,channel,before,limit+1], |row| { let sender: String = row.get(2)?; Ok(json!({"id":row.get::<_,i64>(0)?,"ts":row.get::<_,String>(1)?,"user":sender,"own":sender == owner,"text":row.get::<_,String>(3)?,"thread_ts":row.get::<_,Option<String>>(4)?,"deleted":row.get::<_,bool>(5)?,"edited":row.get::<_,bool>(6)?})) }).map_err(|e| e.to_string())?;
        let mut messages = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let more = messages.len() as u64 > limit;
        messages.truncate(limit as usize);
        messages.reverse();
        Ok(
            json!({"next_before":if more { messages.first().map(|m| m["ts"].clone()) } else { None },"messages":messages}),
        )
    }
}

fn identity(connection: &Connection) -> Result<(&str, &str), String> {
    Ok((
        connection
            .team_id
            .as_deref()
            .ok_or("请重新验证 Slack 工作区身份")?,
        connection
            .user_id
            .as_deref()
            .ok_or("请重新验证 Slack 用户身份")?,
    ))
}
fn valid_ts(ts: &str) -> bool {
    ts.len() <= 50
        && !ts.is_empty()
        && ts.split('.').count() <= 2
        && ts
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        && ts
            .parse::<f64>()
            .is_ok_and(|value| value.is_finite() && (0.0..=253402300799.0).contains(&value))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn connection() -> Connection {
        serde_json::from_value(json!({"token":"xoxp-private-user","app_token":"xapp-private-app","auth_mode":"slack_user","listen":true,"user_id":"U123","team_id":"T123","enabled":true,"allow_writes":true})).unwrap()
    }
    fn payload(id: &str, event: Value) -> Value {
        json!({"type":"event_callback","team_id":"T123","authorizations":[{"team_id":"T123","user_id":"U123","is_bot":false}],"event_id":id,"event":event})
    }
    fn event(channel: &str, kind: &str, text: &str, ts: &str) -> Value {
        json!({"type":"message","channel":channel,"channel_type":kind,"user":"U456","text":text,"ts":ts})
    }
    #[test]
    fn related_messages_include_dms_mentions_and_their_threads_but_not_other_channel_traffic() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = Inbox::open(&dir.path().join("inbox.db")).unwrap();
        let c = connection();
        assert!(!inbox
            .ingest(
                &c,
                &payload("ordinary", event("C1", "channel", "hello", "100.000001"))
            )
            .unwrap());
        for (id, channel, kind) in [("dm", "D1", "im"), ("group", "G1", "mpim")] {
            assert!(inbox
                .ingest(
                    &c,
                    &payload(id, event(channel, kind, "hello", "100.000002"))
                )
                .unwrap());
        }
        assert!(inbox
            .ingest(
                &c,
                &payload(
                    "mention",
                    event("C1", "channel", "Hi <@U123>", "100.000003")
                )
            )
            .unwrap());
        let mut reply = event("C1", "channel", "thread response", "100.000004");
        reply["thread_ts"] = json!("100.000003");
        assert!(inbox.ingest(&c, &payload("reply", reply.clone())).unwrap());
        reply["channel"] = json!("C2");
        assert!(!inbox.ingest(&c, &payload("other-thread", reply)).unwrap());
        assert_eq!(
            inbox.conversations(&c).unwrap().as_array().unwrap().len(),
            3
        );
        assert_eq!(
            inbox.messages(&c, "C1", None, 50).unwrap()["messages"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn all_scope_still_checks_workspace_user_authorization_and_keeps_accounts_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = Inbox::open(&dir.path().join("inbox.db")).unwrap();
        let mut c = connection();
        c.listen_scope = ListenScope::All;
        let data = payload("first", event("C1", "channel", "ordinary", "100.000001"));
        assert!(inbox.ingest(&c, &data).unwrap());
        for changed in [
            json!({"team_id":"TOTHER"}),
            json!({"authorizations":[{"user_id":"UOTHER","is_bot":false}]}),
            json!({"authorizations":[{"user_id":"U123","is_bot":true}]}),
            json!({"authorizations":[]}),
        ] {
            let mut untrusted = data.clone();
            for (key, value) in changed.as_object().unwrap() {
                untrusted[key] = value.clone();
            }
            assert!(!inbox.ingest(&c, &untrusted).unwrap());
        }
        c.user_id = Some("UOTHER".into());
        assert_eq!(inbox.conversations(&c).unwrap(), json!([]));
        assert_eq!(
            inbox.messages(&c, "C1", None, 50).unwrap()["messages"],
            json!([])
        );
    }
    #[test]
    fn duplicate_events_edits_deletes_and_out_of_order_delivery_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inbox.db");
        let inbox = Inbox::open(&path).unwrap();
        let c = connection();
        let original = payload("original", event("D1", "im", "first", "100.000001"));
        assert!(inbox.ingest(&c, &original).unwrap());
        assert!(!inbox.ingest(&c, &original).unwrap());
        let change = payload(
            "edited",
            json!({"type":"message","subtype":"message_changed","channel":"D1","channel_type":"im","event_ts":"101.000001","message":{"ts":"100.000001","user":"U456","text":"updated"}}),
        );
        inbox.ingest(&c, &change).unwrap();
        assert_eq!(
            inbox.messages(&c, "D1", None, 50).unwrap()["messages"][0]["text"],
            "updated"
        );
        let delete = payload(
            "deleted",
            json!({"type":"message","subtype":"message_deleted","channel":"D1","channel_type":"im","event_ts":"102.000001","deleted_ts":"100.000001","previous_message":{"ts":"100.000001","user":"U456","text":"updated"}}),
        );
        inbox.ingest(&c, &delete).unwrap();
        let mut old = change;
        old["event_id"] = json!("late-edit");
        inbox.ingest(&c, &old).unwrap();
        drop(inbox);
        let reopened = Inbox::open(&path).unwrap();
        assert!(!reopened.ingest(&c, &original).unwrap());
        let result = reopened.messages(&c, "D1", None, 50).unwrap();
        assert_eq!(result["messages"].as_array().unwrap().len(), 1);
        assert_eq!(result["messages"][0]["deleted"], true);
        assert_eq!(result["messages"][0]["text"], "");
    }
    #[test]
    fn sent_messages_deduplicate_socket_echoes_and_pagination_keeps_every_message() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = Inbox::open(&dir.path().join("inbox.db")).unwrap();
        let c = connection();
        inbox
            .sent(
                &c,
                &json!({"channel":"D1","text":"sent"}),
                &json!({"data":{"ts":"100.000001"}}),
            )
            .unwrap();
        let mut echo = event("D1", "im", "sent", "100.000001");
        echo["user"] = json!("U123");
        inbox.ingest(&c, &payload("echo", echo)).unwrap();
        for number in 2..=120 {
            inbox
                .ingest(
                    &c,
                    &payload(
                        &format!("m{number}"),
                        event("D1", "im", "message", &format!("100.{number:06}")),
                    ),
                )
                .unwrap();
        }
        let mut before = None;
        let mut count = 0;
        loop {
            let page = inbox.messages(&c, "D1", before.as_deref(), 50).unwrap();
            count += page["messages"].as_array().unwrap().len();
            before = page["next_before"].as_str().map(str::to_owned);
            if before.is_none() {
                break;
            }
        }
        assert_eq!(count, 120);
        assert!(inbox.messages(&c, "D1", Some("bad"), 50).is_err());
        assert!(inbox.messages(&c, "D1", None, 101).is_err());
        assert!(!inbox
            .ingest(
                &c,
                &payload(
                    "bad-ts",
                    event("D1", "im", "bad", "9999999999999999999999999999")
                )
            )
            .unwrap());
    }
}
