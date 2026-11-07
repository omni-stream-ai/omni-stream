use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub updated_at: i64,
    pub id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub cursor: Option<Cursor>,
}

fn default_limit() -> usize {
    20
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub id: String,
    pub title: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub items: Vec<Summary>,
    pub next_cursor: Option<Cursor>,
    pub total: usize,
}

pub fn schema(connection: &Connection) -> Result<(), String> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='conversation_activity')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Ok(());
    }
    let tx = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS conversation_activity (id TEXT PRIMARY KEY, updated_at INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS conversation_activity_order ON conversation_activity(updated_at DESC,id DESC);
         INSERT OR IGNORE INTO conversation_activity(id,updated_at)
             SELECT project,0 FROM conversations UNION SELECT id,0 FROM conversation_settings;",
    )
    .map_err(|e| e.to_string())?;
    // Triggers also cover messages written by the automation transaction.
    for (table, key, other_table, other_key) in [
        ("conversations", "project", "conversation_settings", "id"),
        ("conversation_settings", "id", "conversations", "project"),
    ] {
        for event in ["INSERT", "UPDATE"] {
            tx.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS {table}_activity_{event} AFTER {event} ON {table}
                 BEGIN
                     INSERT INTO conversation_activity(id,updated_at)
                     VALUES (NEW.{key},MAX(CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
                         COALESCE((SELECT MAX(updated_at)+1 FROM conversation_activity),0)))
                     ON CONFLICT(id) DO UPDATE SET updated_at=excluded.updated_at;
                 END;"
            ))
            .map_err(|e| e.to_string())?;
        }
        tx.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS {table}_activity_delete AFTER DELETE ON {table}
             BEGIN
                 DELETE FROM conversation_activity WHERE id=OLD.{key}
                     AND NOT EXISTS(SELECT 1 FROM {other_table} WHERE {other_key}=OLD.{key});
             END;"
        ))
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn list(connection: &Connection, request: &Request) -> Result<Page, String> {
    if !(1..=100).contains(&request.limit) {
        return Err("limit 必须在 1 到 100 之间".into());
    }
    let tx = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let total = tx
        .query_row(
            "SELECT COUNT(*) FROM conversation_activity a
             LEFT JOIN conversation_settings s ON s.id=a.id
             WHERE COALESCE(json_extract(s.data,'$.archived'),0)=0",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut statement = tx.prepare(
        "SELECT p.id,p.updated_at,json_extract(s.data,'$.title'),
            COALESCE(
                (SELECT json_extract(value,'$.content') FROM json_each(c.data)
                 WHERE json_extract(value,'$.role')='assistant' AND json_type(value,'$.content')='text'
                     AND length(trim(json_extract(value,'$.content')))>0 LIMIT 1),
                (SELECT json_extract(value,'$.content') FROM json_each(c.data)
                 WHERE json_extract(value,'$.role')='user' AND json_type(value,'$.content')='text'
                     AND length(trim(json_extract(value,'$.content')))>0 LIMIT 1))
         FROM (SELECT a.id,a.updated_at FROM conversation_activity a
             LEFT JOIN conversation_settings s ON s.id=a.id
             WHERE COALESCE(json_extract(s.data,'$.archived'),0)=0
                 AND (?1 IS NULL OR (a.updated_at,a.id)<(?1,?2))
             ORDER BY a.updated_at DESC,a.id DESC LIMIT ?3) p
         LEFT JOIN conversations c ON c.project=p.id
         LEFT JOIN conversation_settings s ON s.id=p.id
         ORDER BY p.updated_at DESC,p.id DESC"
    ).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(
            params![
                request.cursor.as_ref().map(|cursor| cursor.updated_at),
                request.cursor.as_ref().map(|cursor| &cursor.id),
                request.limit + 1
            ],
            |row| {
                let title: Option<String> = row.get(2)?;
                let preview: Option<String> = row.get(3)?;
                Ok(Summary {
                    id: row.get(0)?,
                    updated_at: row.get(1)?,
                    title: title.filter(|title| !title.trim().is_empty()).or_else(|| {
                        let preview = preview?;
                        let plain: String = preview
                            .chars()
                            .filter(|c| !"#*_`>[]".contains(*c))
                            .collect();
                        plain
                            .lines()
                            .map(str::trim)
                            .find(|line| !line.is_empty())
                            .map(str::to_string)
                    }),
                })
            },
        )
        .map_err(|e| e.to_string())?;
    let mut items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let has_more = items.len() > request.limit;
    items.truncate(request.limit);
    let next_cursor = items.last().filter(|_| has_more).map(|item| Cursor {
        updated_at: item.updated_at,
        id: item.id.clone(),
    });
    drop(statement);
    tx.commit().map_err(|e| e.to_string())?;
    Ok(Page {
        items,
        next_cursor,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{conversation_settings::Settings, storage::Store};
    use serde_json::json;

    fn first_page(store: &Store) -> Page {
        store
            .conversation_page(&Request {
                limit: 20,
                cursor: None,
            })
            .unwrap()
    }

    #[test]
    fn writes_reorder_sessions_and_reads_and_restarts_preserve_order() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("db");
        let store = Store::open(&path).unwrap();
        store
            .append_message("z", &json!({"role":"user","content":"first"}))
            .unwrap();
        store
            .save_conversation_settings("a", &Default::default())
            .unwrap();
        assert_eq!(store.conversations().unwrap(), ["a", "z"]);
        store
            .save_conversation(
                "z",
                &[json!({"role":"assistant","content":"# **Result**\nMore text"})],
            )
            .unwrap();
        let page = first_page(&store);
        assert_eq!(page.items[0].id, "z");
        assert_eq!(page.items[0].title.as_deref(), Some("Result"));
        assert!(page.items[0].updated_at > page.items[1].updated_at);
        let timestamp = page.items[0].updated_at;
        store.conversation("a").unwrap();
        store.conversation_settings("a").unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.conversations().unwrap(), ["z", "a"]);
        assert_eq!(first_page(&store).items[0].updated_at, timestamp);
        store.rename_conversation("a", "  Custom title  ").unwrap();
        assert_eq!(
            first_page(&store).items[0].title.as_deref(),
            Some("Custom title")
        );
        store
            .append_message("z", &json!({"role":"user","content":"continue"}))
            .unwrap();
        assert_eq!(first_page(&store).items[0].id, "z");
    }

    #[test]
    fn cursor_pages_filter_archives_and_handle_equal_timestamps_and_deleted_boundaries() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("db");
        let store = Store::open(&path).unwrap();
        for index in 0..45 {
            store
                .save_conversation_settings(
                    &format!("session-{index:02}"),
                    &Settings {
                        title: Some(format!("Title {index}")),
                        archived: index == 25,
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("UPDATE conversation_activity SET updated_at=100", [])
            .unwrap();
        let first = first_page(&store);
        assert_eq!(first.total, 44);
        assert_eq!(first.items.len(), 20);
        assert_eq!(first.items.first().unwrap().id, "session-44");
        assert_eq!(first.items.last().unwrap().id, "session-24");
        store.delete_conversation("session-24").unwrap();
        let second = store
            .conversation_page(&Request {
                limit: 20,
                cursor: first.next_cursor,
            })
            .unwrap();
        assert_eq!(second.items.len(), 20);
        assert_eq!(second.items[0].id, "session-23");
        let third = store
            .conversation_page(&Request {
                limit: 20,
                cursor: second.next_cursor,
            })
            .unwrap();
        assert_eq!(third.items.len(), 4);
        assert!(third.next_cursor.is_none());
        let ids: std::collections::HashSet<_> = first
            .items
            .iter()
            .chain(&second.items)
            .chain(&third.items)
            .map(|item| &item.id)
            .collect();
        assert_eq!(ids.len(), 44);
        assert!(!ids.contains(&"session-25".to_string()));
        assert!(store
            .conversation_page(&Request {
                limit: 0,
                cursor: None
            })
            .is_err());
        assert!(store
            .conversation_page(&Request {
                limit: 101,
                cursor: None
            })
            .is_err());
    }

    #[test]
    fn migration_keeps_legacy_sessions_and_tracks_writes_from_other_connections() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("db");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(
            "CREATE TABLE conversations (project TEXT PRIMARY KEY,data TEXT NOT NULL);
             CREATE TABLE conversation_settings (id TEXT PRIMARY KEY,data TEXT NOT NULL);
             INSERT INTO conversations VALUES ('history-only','[{\"role\":\"user\",\"content\":\"Legacy message\"}]');
             INSERT INTO conversation_settings VALUES ('settings-only','{\"title\":\"Legacy title\"}');
             INSERT INTO conversation_settings VALUES ('history-only','{}');"
        ).unwrap();
        let store = Store::open(&path).unwrap();
        let page = first_page(&store);
        assert_eq!(page.total, 2);
        assert!(store.conversation_exists("history-only").unwrap());
        assert!(store.conversation_exists("settings-only").unwrap());
        assert!(!store.conversation_exists("draft").unwrap());
        assert!(page.items.iter().all(|item| item.updated_at == 0));
        assert_eq!(page.items[0].title.as_deref(), Some("Legacy title"));
        assert_eq!(page.items[1].title.as_deref(), Some("Legacy message"));
        connection
            .execute(
                "UPDATE conversations SET data='[]' WHERE project='history-only'",
                [],
            )
            .unwrap();
        assert_eq!(first_page(&store).items[0].id, "history-only");
        assert!(first_page(&store).items[0].updated_at > 0);
        store.delete_conversation("history-only").unwrap();
        store.delete_conversation("settings-only").unwrap();
        assert_eq!(first_page(&store).total, 0);
        assert!(!store.conversation_exists("history-only").unwrap());
        assert!(first_page(&store).next_cursor.is_none());
    }
}
