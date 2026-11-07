use crate::AgentExecution;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;

pub use omnistream_core::Approval;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub name: String,
    pub path: String,
}

pub struct Store(Connection);
impl Store {
    pub fn notification_snapshot(&self, conversation: &str) -> Result<serde_json::Value, String> {
        crate::notifications::snapshot(
            &self.0,
            serde_json::json!(["chat", conversation]).to_string(),
        )
    }
    pub fn claim_approved_plugin(&self, id: i64) -> Result<bool, String> {
        self.0.execute("UPDATE approvals SET status='executing' WHERE id=?1 AND action='plugin.invoke' AND status='approved'", [id])
            .map(|count|count==1).map_err(|e|e.to_string())
    }
    pub fn expire_plugin_approvals(&self) -> Result<(), String> {
        self.0.execute("UPDATE approvals SET status='expired' WHERE action='plugin.invoke' AND status IN ('pending','approved','executing')",[])
            .map(|_|()).map_err(|e|e.to_string())
    }
    pub fn expire_plugin_approvals_for_task(&self, task_id: u64) -> Result<(), String> {
        self.0.execute("UPDATE approvals SET status='expired' WHERE task_id=?1 AND action='plugin.invoke' AND status IN ('pending','approved','executing')", [task_id])
            .map(|_| ()).map_err(|e| e.to_string())
    }
    pub fn claim_approval(&self, id: i64) -> Result<bool, String> {
        self.0
            .execute(
                "UPDATE approvals SET status='executing' WHERE id=?1 AND status='pending'",
                [id],
            )
            .map(|n| n == 1)
            .map_err(|e| e.to_string())
    }
    pub fn finish_approval(&self, id: i64, result: &Result<String, String>) -> Result<(), String> {
        let (status, output) = match result {
            Ok(s) => ("succeeded", s),
            Err(e) => ("failed", e),
        };
        let tx = self.0.unchecked_transaction().map_err(|e| e.to_string())?;
        if tx
            .execute(
                "UPDATE approvals SET status=?1 WHERE id=?2 AND status='executing'",
                params![status, id],
            )
            .map_err(|e| e.to_string())?
            != 1
        {
            return Err("审批并非执行中状态".into());
        }
        tx.execute(
            "INSERT INTO events(task_id,kind,data) VALUES (0,'approval.finished',?1)",
            [serde_json::json!({"id":id,"status":status,"output":output}).to_string()],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS tasks (id INTEGER PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                kind TEXT NOT NULL, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS conversations (project TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS conversation_settings (id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS conversation_calls (task_id INTEGER PRIMARY KEY, conversation TEXT NOT NULL, scope TEXT NOT NULL, agent TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS conversation_calls_v2 (task_id INTEGER NOT NULL, conversation TEXT NOT NULL, scope TEXT NOT NULL, agent TEXT NOT NULL, PRIMARY KEY(task_id, conversation));
            INSERT OR IGNORE INTO conversation_calls_v2(task_id,conversation,scope,agent) SELECT task_id,conversation,scope,agent FROM conversation_calls;
            CREATE TABLE IF NOT EXISTS agent_sessions (scope TEXT PRIMARY KEY, session_id TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS projects (path TEXT PRIMARY KEY, name TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
            CREATE TABLE IF NOT EXISTS approvals (id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER, action TEXT NOT NULL, payload TEXT NOT NULL,
                status TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
            CREATE TABLE IF NOT EXISTS scripts (id INTEGER PRIMARY KEY AUTOINCREMENT,
                project TEXT NOT NULL, name TEXT NOT NULL, path TEXT NOT NULL,
                version INTEGER NOT NULL, content_hash TEXT NOT NULL, content TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);",
        )
        .map_err(|e| e.to_string())?;
        crate::conversation_list::schema(&conn)?;
        crate::notifications::schema(&conn)?;
        crate::push::schema(&conn)?;
        Ok(Self(conn))
    }
    pub fn register_project(&self, path: &Path) -> Result<(), String> {
        self.register_named_project(path, None).map(|_| ())
    }
    pub fn agent_session(&self, scope: &str) -> Result<Option<String>, String> {
        self.0
            .query_row(
                "SELECT session_id FROM agent_sessions WHERE scope=?1",
                [scope],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }
    pub fn save_agent_session(&self, scope: &str, id: &str) -> Result<(), String> {
        self.0.execute("INSERT INTO agent_sessions(scope,session_id) VALUES (?1,?2) ON CONFLICT(scope) DO UPDATE SET session_id=excluded.session_id", params![scope,id]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn agent_event(&self, task_id: u64, event: &serde_json::Value) -> Result<(), String> {
        self.0
            .execute(
                "INSERT INTO events(task_id,kind,data) VALUES (?1,'agent.event',?2)",
                params![task_id, event.to_string()],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn expire_agent_approval(&self, id: i64) -> Result<(), String> {
        self.0.execute("UPDATE approvals SET status='expired' WHERE id=?1 AND status IN ('pending','approved','rejected')", [id]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn register_named_project(
        &self,
        path: &Path,
        name: Option<&str>,
    ) -> Result<Project, String> {
        let path = path
            .canonicalize()
            .map_err(|e| format!("项目目录无效：{e}"))?;
        if !path.is_dir() {
            return Err("项目路径不是目录".into());
        }
        let explicit_name = name.map(str::trim);
        if explicit_name == Some("") {
            return Err("项目名称不能为空".into());
        }
        let path = path.to_string_lossy().to_string();
        let default_name = Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(&path)
            .to_string();
        let name = explicit_name.unwrap_or(&default_name);
        self.0.execute(
            "INSERT INTO projects(path,name) VALUES (?1,?2) ON CONFLICT(path) DO UPDATE SET name=CASE WHEN ?3 THEN excluded.name ELSE projects.name END, updated_at=CURRENT_TIMESTAMP",
            params![path, name, explicit_name.is_some()],
        ).map_err(|e| e.to_string())?;
        self.0
            .query_row(
                "SELECT name,path FROM projects WHERE path=?1",
                [path],
                |row| {
                    Ok(Project {
                        name: row.get(0)?,
                        path: row.get(1)?,
                    })
                },
            )
            .map_err(|e| e.to_string())
    }
    pub fn projects(&self) -> Result<Vec<Project>, String> {
        let mut stmt = self
            .0
            .prepare("SELECT name,path FROM projects ORDER BY name,path")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Project {
                    name: row.get(0)?,
                    path: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row| row.map_err(|e| e.to_string())).collect()
    }
    pub fn rename_project(&self, path: &str, name: &str) -> Result<Project, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("项目名称不能为空".into());
        }
        let changed = self
            .0
            .execute(
                "UPDATE projects SET name=?1,updated_at=CURRENT_TIMESTAMP WHERE path=?2",
                params![name, path],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("项目未登记".into());
        }
        self.0
            .query_row(
                "SELECT name,path FROM projects WHERE path=?1",
                [path],
                |row| {
                    Ok(Project {
                        name: row.get(0)?,
                        path: row.get(1)?,
                    })
                },
            )
            .map_err(|e| e.to_string())
    }
    pub fn delete_project(&self, path: &str) -> Result<(), String> {
        let changed = self
            .0
            .execute("DELETE FROM projects WHERE path=?1", [path])
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("项目未登记".into());
        }
        Ok(())
    }
    pub fn load(&self) -> Result<Vec<AgentExecution>, String> {
        let mut stmt = self
            .0
            .prepare("SELECT data FROM tasks ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        })
        .collect()
    }
    #[cfg(test)]
    pub fn events(&self, task_id: u64) -> Result<Vec<serde_json::Value>, String> {
        let mut statement = self
            .0
            .prepare("SELECT id,created_at,kind,data FROM events WHERE task_id=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = statement.query_map([task_id], |row| { let data: String = row.get(3)?; Ok(serde_json::json!({"id":row.get::<_,i64>(0)?,"created_at":row.get::<_,String>(1)?,"kind":row.get::<_,String>(2)?,"data":serde_json::from_str::<serde_json::Value>(&data).unwrap_or(serde_json::Value::Null)})) }).map_err(|e| e.to_string())?;
        rows.map(|row| row.map_err(|e| e.to_string())).collect()
    }
    pub fn events_after(&self, id: i64) -> Result<Vec<serde_json::Value>, String> {
        let mut statement = self
            .0
            .prepare("SELECT id,created_at,kind,data FROM events WHERE id>?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = statement.query_map([id], |row| { let data: String = row.get(3)?; Ok(serde_json::json!({"id":row.get::<_,i64>(0)?,"created_at":row.get::<_,String>(1)?,"kind":row.get::<_,String>(2)?,"data":serde_json::from_str::<serde_json::Value>(&data).unwrap_or(serde_json::Value::Null)})) }).map_err(|e| e.to_string())?;
        rows.map(|row| row.map_err(|e| e.to_string())).collect()
    }
    pub fn conversation_exists(&self, id: &str) -> Result<bool, String> {
        self.0
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_activity WHERE id=?1)",
                [id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }
    pub fn conversation(&self, project: &str) -> Result<Vec<serde_json::Value>, String> {
        use rusqlite::OptionalExtension;
        let data: Option<String> = self
            .0
            .query_row(
                "SELECT data FROM conversations WHERE project=?1",
                [project],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        data.map(|d| serde_json::from_str(&d).map_err(|e| e.to_string()))
            .unwrap_or_else(|| Ok(Vec::new()))
    }
    pub fn setting(&self, key: &str) -> Result<Option<serde_json::Value>, String> {
        let data: Option<String> = self
            .0
            .query_row("SELECT data FROM app_settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        data.map(|value| serde_json::from_str(&value).map_err(|e| e.to_string()))
            .transpose()
    }
    pub fn save_setting(&self, key: &str, value: &serde_json::Value) -> Result<(), String> {
        self.0.execute("INSERT INTO app_settings(key,data) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET data=excluded.data", params![key,value.to_string()]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn conversation_settings(
        &self,
        id: &str,
    ) -> Result<crate::conversation_settings::Settings, String> {
        let data: Option<String> = self
            .0
            .query_row(
                "SELECT data FROM conversation_settings WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        data.map(|value| serde_json::from_str(&value).map_err(|e| e.to_string()))
            .unwrap_or_else(|| Ok(Default::default()))
    }
    pub fn rename_conversation(
        &self,
        id: &str,
        title: &str,
    ) -> Result<crate::conversation_settings::Settings, String> {
        let mut settings = self.conversation_settings(id)?;
        let title = title.trim();
        if title.is_empty() {
            return Err("会话名称不能为空".into());
        }
        settings.title = Some(title.to_string());
        self.save_conversation_settings(id, &settings)?;
        Ok(settings)
    }
    pub fn set_conversation_archived(
        &self,
        id: &str,
        archived: bool,
    ) -> Result<crate::conversation_settings::Settings, String> {
        let mut settings = self.conversation_settings(id)?;
        settings.archived = archived;
        self.save_conversation_settings(id, &settings)?;
        Ok(settings)
    }
    pub fn delete_conversation(&self, id: &str) -> Result<(), String> {
        let tx = self.0.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM conversations WHERE project=?1", [id])
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM conversation_settings WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM conversation_calls_v2 WHERE conversation=?1",
            [id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM agent_sessions WHERE scope LIKE ?1",
            [format!("conversation:{id}:%")],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn append_message(&self, id: &str, message: &serde_json::Value) -> Result<(), String> {
        self.0.execute("INSERT INTO conversations(project,data) VALUES (?1,json_array(json(?2))) ON CONFLICT(project) DO UPDATE SET data=json_insert(conversations.data,'$[#]',json(?2))", params![id,message.to_string()]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn save_call(
        &self,
        task_id: u64,
        conversation: &str,
        scope: &str,
        agent: &str,
    ) -> Result<(), String> {
        self.0.execute("INSERT OR IGNORE INTO conversation_calls_v2(task_id,conversation,scope,agent) VALUES (?1,?2,?3,?4)", params![task_id,conversation,scope,agent]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn conversation_calls(&self, conversation: &str) -> Result<Vec<serde_json::Value>, String> {
        let mut statement = self.0.prepare("SELECT c.task_id,c.scope,c.agent,t.data FROM conversation_calls_v2 c JOIN tasks t ON t.id=c.task_id WHERE c.conversation=?1 AND json_extract(t.data,'$.transient')=1 ORDER BY c.task_id").map_err(|e| e.to_string())?;
        let rows = statement.query_map([conversation], |row| {
            let task: String = row.get(3)?;
            Ok(serde_json::json!({"task_id":row.get::<_,u64>(0)?,"scope":row.get::<_,String>(1)?,"agent":row.get::<_,String>(2)?,"task":serde_json::from_str::<serde_json::Value>(&task).unwrap_or_default()}))
        }).map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }
    pub fn scope_events(&self, scope: &str) -> Result<Vec<serde_json::Value>, String> {
        let mut statement = self.0.prepare("SELECT data FROM events WHERE kind='agent.event' AND json_extract(data,'$.scope')=?1 ORDER BY id").map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([scope], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|r| {
            r.map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        })
        .collect()
    }
    pub fn save_conversation_settings(
        &self,
        id: &str,
        settings: &crate::conversation_settings::Settings,
    ) -> Result<(), String> {
        if id.trim().is_empty() {
            return Err("会话标识不能为空".into());
        }
        let data = serde_json::to_string(settings).map_err(|e| e.to_string())?;
        self.0.execute("INSERT INTO conversation_settings(id,data) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data", params![id,data]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn conversations(&self) -> Result<Vec<String>, String> {
        let mut statement = self
            .0
            .prepare("SELECT id FROM conversation_activity ORDER BY updated_at DESC,id DESC")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|row| row.map_err(|e| e.to_string())).collect()
    }
    pub fn conversation_page(
        &self,
        request: &crate::conversation_list::Request,
    ) -> Result<crate::conversation_list::Page, String> {
        crate::conversation_list::list(&self.0, request)
    }
    pub fn save_conversation(
        &self,
        project: &str,
        history: &[serde_json::Value],
    ) -> Result<(), String> {
        let data = serde_json::to_string(history).map_err(|e| e.to_string())?;
        self.0.execute("INSERT INTO conversations(project,data) VALUES (?1,?2) ON CONFLICT(project) DO UPDATE SET data=excluded.data", params![project,data]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn save(&mut self, task: &AgentExecution) -> Result<(), String> {
        let data = serde_json::to_string(task).map_err(|e| e.to_string())?;
        // Acquire the writer lock before reading: a deferred WAL snapshot cannot
        // be upgraded after another writer commits, regardless of busy_timeout.
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT json_extract(data,'$.status') FROM tasks WHERE id=?1",
                [task.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO tasks(id,data) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data", params![task.id, data]).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO events(task_id,kind,data) VALUES (?1,'task.saved',?2)",
            params![task.id, data],
        )
        .map_err(|e| e.to_string())?;
        if task.transient
            && previous
                .as_deref()
                .is_some_and(|status| status != task.status)
            && matches!(
                task.status.as_str(),
                "unverified" | "succeeded" | "completed" | "failed" | "interrupted"
            )
        {
            let conversation: Option<String> = tx.query_row(
                "SELECT conversation FROM conversation_calls_v2 WHERE task_id=?1 ORDER BY conversation LIMIT 1",
                [task.id], |row| row.get(0),
            ).optional().map_err(|e| e.to_string())?;
            if let Some(conversation) = conversation {
                crate::notifications::insert(
                    &tx,
                    &format!("call:{}", task.id),
                    if matches!(task.status.as_str(), "failed" | "interrupted") {
                        "task_failed"
                    } else {
                        "task_completed"
                    },
                    &task.title,
                    &conversation,
                )?;
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn create_approval(
        &self,
        task_id: Option<u64>,
        action: &str,
        payload: &serde_json::Value,
    ) -> Result<i64, String> {
        let data = serde_json::to_string(payload).map_err(|e| e.to_string())?;
        let tx = self.0.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO approvals(task_id,action,payload,status) VALUES (?1,?2,?3,'pending')",
            params![task_id, action, data],
        )
        .map_err(|e| e.to_string())?;
        let id = tx.last_insert_rowid();
        let conversation = if let Some(id) = payload["conversation"].as_str() {
            Some(id.to_string())
        } else {
            tx.query_row("SELECT conversation FROM conversation_calls_v2 WHERE task_id=?1 ORDER BY conversation LIMIT 1",
                [task_id], |row| row.get::<_,String>(0)).optional().map_err(|e|e.to_string())?
        };
        if let Some(conversation) = conversation {
            crate::notifications::insert(
                &tx,
                &format!("approval:{id}"),
                "approval_pending",
                "",
                &conversation,
            )?;
            tx.execute(
                "UPDATE notifications SET approval_id=?1 WHERE event_key=?2",
                params![id, format!("approval:{id}")],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub fn notifications(
        &self,
        input: crate::notifications::Request,
        slack: Option<&crate::plugin_connections::Connection>,
    ) -> Result<serde_json::Value, String> {
        crate::notifications::poll(&self.0, input, slack)
    }
    pub fn approvals(&self, status: Option<&str>) -> Result<Vec<Approval>, String> {
        let mut statement = self.0.prepare("SELECT id,task_id,action,payload,status FROM approvals WHERE (?1 IS NULL OR status=?1) ORDER BY id").map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([status], |row| {
                let payload: String = row.get(3)?;
                Ok(Approval {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    action: row.get(2)?,
                    payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
                    status: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row| row.map_err(|e| e.to_string())).collect()
    }
    pub fn resolve_approval(&self, id: i64, status: &str) -> Result<bool, String> {
        if !matches!(status, "approved" | "rejected") {
            return Err("非法审批状态".into());
        }
        Ok(self
            .0
            .execute(
                "UPDATE approvals SET status=?1 WHERE id=?2 AND status='pending'",
                params![status, id],
            )
            .map_err(|e| e.to_string())?
            == 1)
    }
    pub fn respond_approval(&self, id: i64, response: &serde_json::Value) -> Result<bool, String> {
        let item = self.approval(id)?.ok_or("审批不存在")?;
        if item.action != "agent.request" {
            return Err("此审批不接受结构化响应".into());
        }
        let request: crate::adapter::ApprovalRequest =
            serde_json::from_value(item.payload["request"].clone()).map_err(|e| e.to_string())?;
        crate::server_requests::validate(&request, response)?;
        let mut envelope = item.payload;
        envelope["response"] = response.clone();
        let payload = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
        Ok(self.0.execute("UPDATE approvals SET status='approved', payload=?1 WHERE id=?2 AND status='pending'", params![payload,id]).map_err(|e| e.to_string())? == 1)
    }
    pub fn approval(&self, id: i64) -> Result<Option<Approval>, String> {
        use rusqlite::OptionalExtension;
        self.0
            .query_row(
                "SELECT id,task_id,action,payload,status FROM approvals WHERE id=?1",
                [id],
                |row| {
                    let payload: String = row.get(3)?;
                    Ok(Approval {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        action: row.get(2)?,
                        payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
                        status: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
    }
    pub fn record_script(&self, project: &str, name: &str, path: &Path) -> Result<bool, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        let previous: Option<(String, i64)> = self.0.query_row("SELECT content_hash,version FROM scripts WHERE project=?1 AND name=?2 ORDER BY version DESC LIMIT 1", params![project,name], |row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(|e| e.to_string())?;
        if previous
            .as_ref()
            .map(|(old, _)| old == &hash)
            .unwrap_or(false)
        {
            return Ok(false);
        }
        let version = previous.map(|(_, version)| version + 1).unwrap_or(1);
        self.0.execute("INSERT INTO scripts(project,name,path,version,content_hash,content) VALUES (?1,?2,?3,?4,?5,?6)", params![project,name,path.to_string_lossy(),version,hash,content]).map_err(|e| e.to_string())?;
        Ok(true)
    }
    pub fn script_history(
        &self,
        project: &str,
        name: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        let mut stmt = self.0.prepare("SELECT version,content_hash,path,created_at FROM scripts WHERE project=?1 AND name=?2 ORDER BY version").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![project,name], |row| Ok(serde_json::json!({"version":row.get::<_,i64>(0)?,"content_hash":row.get::<_,String>(1)?,"path":row.get::<_,String>(2)?,"created_at":row.get::<_,String>(3)?}))).map_err(|e| e.to_string())?;
        rows.map(|row| row.map_err(|e| e.to_string())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_save_waits_for_a_writer_before_reading_its_status() {
        use std::{
            sync::atomic::{AtomicBool, Ordering},
            time::{Duration, Instant},
        };
        static WAITING_FOR_WRITER: AtomicBool = AtomicBool::new(false);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let mut store = Store::open(&path).unwrap();
        let mut task = crate::App::default().create_execution("demo", "concurrent", "goal");
        store.save(&task).unwrap();
        store.save_call(task.id, "chat", "scope", "agent").unwrap();
        store
            .0
            .busy_handler(Some(|attempt| {
                WAITING_FOR_WRITER.store(true, Ordering::Release);
                std::thread::sleep(Duration::from_millis(1));
                attempt < 5000
            }))
            .unwrap();

        let mut other = Connection::open(&path).unwrap();
        let writer = other
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        writer
            .execute(
                "INSERT INTO app_settings(key,data) VALUES('concurrent','true')",
                [],
            )
            .unwrap();
        task.status = "unverified".into();
        let saving = std::thread::spawn(move || store.save(&task));

        // Release the competing writer only after save is blocked or has failed.
        // A deferred read-then-write transaction fails without invoking busy_handler.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !WAITING_FOR_WRITER.load(Ordering::Acquire)
            && !saving.is_finished()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        writer.commit().unwrap();
        saving.join().unwrap().unwrap();
        assert!(WAITING_FOR_WRITER.load(Ordering::Acquire));

        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.load().unwrap()[0].status, "unverified");
        assert_eq!(
            reopened.setting("concurrent").unwrap(),
            Some(serde_json::json!(true))
        );
        assert_eq!(
            reopened
                .events_after(0)
                .unwrap()
                .iter()
                .filter(|event| event["kind"] == "task.saved")
                .count(),
            2
        );
        let notifications: i64 = reopened
            .0
            .query_row(
                "SELECT COUNT(*) FROM notifications WHERE event_key='call:1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(notifications, 1);
    }

    #[test]
    fn conversation_preferences_survive_restart_without_leaking() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("sessions.db");
        let store = Store::open(&db).unwrap();
        let settings = crate::conversation_settings::Settings {
            title: Some("工作会话".into()),
            approval_mode: crate::config::ApprovalMode::All,
            ..Default::default()
        };
        store.save_conversation_settings("a", &settings).unwrap();
        store
            .append_message("a", &serde_json::json!({"role":"user","content":"hello"}))
            .unwrap();
        let other = Store::open(&db).unwrap();
        other
            .append_message(
                "a",
                &serde_json::json!({"role":"assistant","content":"world"}),
            )
            .unwrap();
        drop(store);
        let reopened = Store::open(&db).unwrap();
        assert_eq!(
            reopened.conversation_settings("a").unwrap().approval_mode,
            crate::config::ApprovalMode::All
        );
        assert_eq!(
            reopened.conversation_settings("b").unwrap().approval_mode,
            crate::config::ApprovalMode::Default
        );
        assert_eq!(reopened.conversation("a").unwrap().len(), 2);
        assert!(reopened.conversation("b").unwrap().is_empty());
    }
    #[test]
    fn project_registration_is_persistent_and_preserves_custom_names() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("repo");
        std::fs::create_dir(&project).unwrap();
        let db = dir.path().join("state.db");
        let store = Store::open(&db).unwrap();
        let registered = store.register_named_project(&project, Some(" A ")).unwrap();
        assert_eq!(registered.name, "A");
        assert_eq!(
            registered.path,
            project.canonicalize().unwrap().to_string_lossy()
        );
        store.register_project(&project.join(".")).unwrap();
        drop(store);
        let store = Store::open(&db).unwrap();
        let projects = store.projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "A");
        assert!(store.register_named_project(&project, Some(" ")).is_err());
        assert!(store.register_project(&dir.path().join("missing")).is_err());
        assert!(store.register_project(&db).is_err());
        assert_eq!(store.projects().unwrap().len(), 1);
    }
    #[test]
    fn approval_claim_is_exclusive_across_connections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let a = Store::open(&path).unwrap();
        let b = Store::open(&path).unwrap();
        let id = a
            .create_approval(None, "run_script", &serde_json::json!({}))
            .unwrap();
        assert!(a.claim_approval(id).unwrap());
        assert!(!b.claim_approval(id).unwrap());
        assert!(!b.resolve_approval(id, "rejected").unwrap());
        a.finish_approval(id, &Err("execution failed".into()))
            .unwrap();
        assert_eq!(b.approval(id).unwrap().unwrap().status, "failed");
    }
    #[test]
    fn conversation_is_persistent_and_project_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let history = vec![serde_json::json!({"role":"user","content":"continue"})];
        Store::open(&path)
            .unwrap()
            .save_conversation("a", &history)
            .unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(store.conversation("a").unwrap(), history);
        assert!(store.conversation("b").unwrap().is_empty());
    }
    #[test]
    fn deleting_conversation_removes_all_conversation_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        store
            .append_message(
                "conversation",
                &serde_json::json!({"role":"user","content":"hello"}),
            )
            .unwrap();
        store
            .save_conversation_settings("conversation", &Default::default())
            .unwrap();
        store
            .save_agent_session("conversation:conversation:agent", "thread")
            .unwrap();
        store
            .save_call(
                1,
                "conversation",
                "conversation:conversation:agent",
                "agent",
            )
            .unwrap();

        store.delete_conversation("conversation").unwrap();

        assert!(store.conversation("conversation").unwrap().is_empty());
        assert!(store.conversations().unwrap().is_empty());
        assert!(store
            .agent_session("conversation:conversation:agent")
            .unwrap()
            .is_none());
        assert!(store.conversation_calls("conversation").unwrap().is_empty());
    }
    #[test]
    fn reopen_preserves_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let mut app = crate::App::default();
        let task = app.create_execution("demo", "persistent", "goal");
        Store::open(&path).unwrap().save(&task).unwrap();
        let tasks = Store::open(&path).unwrap().load().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "persistent");
    }
    #[test]
    fn script_history_versions_content_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let script = dir.path().join("check.sh");
        std::fs::write(&script, "one").unwrap();
        let store = Store::open(&path).unwrap();
        assert!(store.record_script("p", "check", &script).unwrap());
        assert!(!store.record_script("p", "check", &script).unwrap());
        std::fs::write(&script, "two").unwrap();
        assert!(store.record_script("p", "check", &script).unwrap());
        assert_eq!(store.script_history("p", "check").unwrap().len(), 2);
    }
}
