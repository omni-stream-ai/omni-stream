use crate::{
    adapter::{
        Adapter, ApprovalDecision, ApprovalRequest, Configured, Event, Host, Request,
        RequestResponse,
    },
    config::{AgentConfig, ApprovalMode},
    storage::Store,
    AgentExecution, Config,
};
use serde_json::json;
use std::{
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub fn scope(owner: &str, project: &str, agent: &AgentConfig) -> String {
    use sha2::{Digest, Sha256};
    let mut identity = json!([
        owner,
        project,
        agent.command,
        agent.args,
        agent.protocol,
        agent.config_file,
        agent.plugin_transport,
        agent.mcp_args,
        agent.plugin_tools
    ]);
    if let Some(model) = &agent.inherited_model {
        identity
            .as_array_mut()
            .unwrap()
            .push(json!(model.identity()));
    }
    format!("{:x}", Sha256::digest(identity.to_string().as_bytes()))
}

/// An OS lock coordinates separate Core processes and is released on process exit.
struct SessionLease {
    _file: std::fs::File,
}
impl SessionLease {
    fn acquire(state: &Path, scope: &str) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        let directory = state
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("agent-locks");
        std::fs::create_dir_all(&directory)
            .map_err(|e| format!("无法创建 Agent 会话锁目录：{e}"))?;
        let name = format!("{:x}.lock", Sha256::digest(scope.as_bytes()));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(name))
            .map_err(|e| format!("无法打开 Agent 会话锁：{e}"))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                Err("该 Agent 会话正在运行，请等待当前执行结束后继续".into())
            }
            Err(std::fs::TryLockError::Error(error)) => {
                Err(format!("无法锁定 Agent 会话：{error}"))
            }
        }
    }
}

struct CoreHost {
    store: Store,
    scope: String,
    task_id: u64,
    cancel: Arc<AtomicBool>,
    approval_mode: ApprovalMode,
    agent_name: String,
    plugin_setup: bool,
}
impl Host for CoreHost {
    fn event(&mut self, mut event: Event) -> Result<(), String> {
        if self.plugin_setup {
            match &mut event {
                Event::Text { .. } => return Ok(()),
                Event::Progress { method, data } => {
                    *method = crate::plugin_setup::redact_text(method);
                    *data = json!({"message":"Plugin setup in progress"});
                }
                Event::Completed { output } => *output = crate::plugin_setup::redact_text(output),
                Event::Failed { message } => *message = crate::plugin_setup::redact_text(message),
                Event::Session { .. } => {}
            }
        }
        if let Event::Session { id } = &event {
            self.store.save_agent_session(&self.scope, id)?;
        }
        if let Event::Text { delta } = &event {
            print!("{delta}");
            let _ = std::io::stdout().flush();
        }
        self.store.agent_event(
            self.task_id,
            &json!({"scope":self.scope,"agent":self.agent_name,"event":event}),
        )
    }
    fn approve(&mut self, request: ApprovalRequest) -> Result<ApprovalDecision, String> {
        match self.begin_request(request)? {
            RequestResponse::Ready(response) => Ok(match response["decision"].as_str() {
                Some("decline") => ApprovalDecision::Decline,
                Some("cancel") => ApprovalDecision::Cancel,
                _ => ApprovalDecision::Accept,
            }),
            RequestResponse::Pending(_) => Err("审批需要结构化响应".into()),
        }
    }
    fn begin_request(&mut self, request: ApprovalRequest) -> Result<RequestResponse, String> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err("审批等待已取消".into());
        }
        let read_only_web = crate::policy::read_only_web(&request);
        if (read_only_web || self.approval_mode == ApprovalMode::All)
            && !matches!(
                request.method.as_str(),
                "item/tool/requestUserInput" | "mcpServer/elicitation/request"
            )
        {
            let response = crate::server_requests::decision(&request, ApprovalDecision::Accept)?;
            if self.approval_mode == ApprovalMode::All
                || crate::server_requests::validate(&request, &response).is_ok()
            {
                self.store.agent_event(
                    self.task_id,
                    &json!({"scope":self.scope,"approval":"auto-approved","method":request.method,"reason":if read_only_web { "read-only-web" } else { "approval-mode-all" }}),
                )?;
                return Ok(RequestResponse::Ready(response));
            }
        }
        let mut public_request = serde_json::to_value(&request).map_err(|e| e.to_string())?;
        if self.plugin_setup {
            crate::plugin_setup::redact_value(&mut public_request);
        }
        let id = self.store.create_approval(
            (self.task_id != 0).then_some(self.task_id),
            "agent.request",
            &json!({"scope":self.scope,"agent":self.agent_name,"request":public_request}),
        )?;
        println!(
            "\nAgent 请求 #{id}：{}\n{}\napproval approve {id} / approval reject {id} / approval respond {id} '{{...}}'",
            public_request["method"].as_str().unwrap_or_default(), public_request["params"]
        );
        let result = (|| loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err("审批等待已取消".to_string());
            }
            match self.store.approval(id)?.ok_or("审批丢失")?.status.as_str() {
                "approved" => {
                    let item = self.store.approval(id)?.ok_or("审批丢失")?;
                    if let Some(response) = item.payload.get("response") {
                        crate::server_requests::validate(&request, response)?;
                        return Ok(RequestResponse::Ready(response.clone()));
                    }
                    if request.method == "item/commandExecution/requestApproval"
                        || request.method == "item/fileChange/requestApproval"
                    {
                        return Ok(RequestResponse::Ready(crate::server_requests::decision(
                            &request,
                            ApprovalDecision::Accept,
                        )?));
                    }
                    crate::server_requests::validate(&request, &item.payload)?;
                    return Ok(RequestResponse::Ready(item.payload));
                }
                "rejected" => {
                    return Ok(RequestResponse::Ready(crate::server_requests::decision(
                        &request,
                        ApprovalDecision::Decline,
                    )?))
                }
                "pending" => std::thread::sleep(Duration::from_millis(50)),
                _ => return Err("审批已失效".into()),
            }
        })();
        self.store.expire_agent_approval(id)?;
        let mut result_log = match &result {
            Ok(RequestResponse::Ready(value)) => value.clone(),
            Ok(RequestResponse::Pending(ticket)) => json!({"pending":ticket}),
            Err(error) => json!({"error":error}),
        };
        if self.plugin_setup {
            crate::plugin_setup::redact_value(&mut result_log);
        }
        self.store.agent_event(
            self.task_id,
            &json!({"scope":self.scope,"approval_id":id,"result":result_log}),
        )?;
        result
    }
}

pub fn run(
    config: &Config,
    agent: &AgentConfig,
    task: &AgentExecution,
    scope: &str,
    cancel: Arc<AtomicBool>,
) -> Result<String, String> {
    let _lease = SessionLease::acquire(&crate::state_path(), scope)?;
    let gateway = crate::agent_plugins::Gateway::start(
        agent,
        config,
        task.conversation.as_deref().unwrap_or(scope),
        scope,
        (task.id != 0).then_some(task.id),
        cancel.clone(),
    )?;
    let prepared = gateway
        .as_ref()
        .map(|gateway| gateway.configure(agent))
        .transpose()?;
    let mut runtime_config = config.clone();
    if let Some(gateway) = &gateway {
        if agent.plugin_transport != crate::agent_plugins::PluginTransport::Command {
            runtime_config.agent_mcp_servers = gateway.acp_servers();
        }
    }
    let runtime_agent = prepared.as_ref().unwrap_or(agent);
    let adapter = Configured {
        agent: runtime_agent,
        config: &runtime_config,
    };
    let capabilities = adapter.capabilities();
    let store = Store::open(&crate::state_path())?;
    let session = if capabilities.resume {
        store.agent_session(scope)?
    } else {
        None
    };
    let mut host = CoreHost {
        store,
        scope: scope.into(),
        task_id: task.id,
        cancel: cancel.clone(),
        approval_mode: config.approval_mode(),
        agent_name: config
            .agents
            .iter()
            .find(|(_, candidate)| {
                candidate.command == agent.command
                    && candidate.args == agent.args
                    && candidate.protocol == agent.protocol
                    && candidate.config_file == agent.config_file
            })
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| agent.command.clone()),
        plugin_setup: config.plugin_setup,
    };
    if let Some(id) = agent.plugin_tools.named_agent {
        host.agent_name = crate::agents::get(id)?.definition.name;
    }
    host.store.agent_event(task.id, &json!({"scope":scope,"capabilities":{"resume":capabilities.resume,"approvals":capabilities.approvals,"streaming":capabilities.streaming}}))?;
    let mut prompt = if let Some(id) = agent.plugin_tools.named_agent {
        let mut prompt = crate::agents::get(id)?.prompt(&task.goal);
        if !config.plugin_setup {
            prompt.push_str(&crate::agent_memory::context(&crate::state_path(), id)?);
        }
        prompt
    } else {
        task.goal.clone()
    };
    prompt.push_str(crate::policy::READ_ONLY_WEB_HINT);
    if let Some(gateway) = &gateway {
        prompt.push_str(&gateway.hint());
    }
    let result = adapter.run(
        Request {
            cwd: Path::new(&task.project),
            prompt: &prompt,
            session,
        },
        &mut host,
        cancel,
    );
    let result = if config.plugin_setup {
        result
            .map(|output| crate::plugin_setup::redact_text(&output))
            .map_err(|error| crate::plugin_setup::redact_text(&error))
    } else {
        result
    };
    if let Err(message) = &result {
        host.event(Event::Failed {
            message: message.clone(),
        })?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn session_leases_isolate_scopes_and_release_without_deleting_the_lock_file() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state.db");
        let first = SessionLease::acquire(&state, "conversation-a").unwrap();
        assert!(SessionLease::acquire(&state, "conversation-a").is_err());
        let other = SessionLease::acquire(&state, "conversation-b").unwrap();
        assert!(SessionLease::acquire(&state, "conversation-b").is_err());
        drop(first);
        assert!(SessionLease::acquire(&state, "conversation-a").is_ok());
        assert!(SessionLease::acquire(&state, "conversation-b").is_err());
        drop(other);
        assert_eq!(
            std::fs::read_dir(directory.path().join("agent-locks"))
                .unwrap()
                .count(),
            2
        );
    }
    #[cfg(unix)]
    #[test]
    fn session_leases_detect_other_processes_and_recover_after_the_owner_dies() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state.db");
        drop(SessionLease::acquire(&state, "conversation").unwrap());
        let lock = std::fs::read_dir(directory.path().join("agent-locks"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        struct Worker(std::process::Child);
        impl Drop for Worker {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut worker = Worker(Command::new("python3").args(["-c", "import fcntl,sys; f=open(sys.argv[1],'a+'); fcntl.flock(f,fcntl.LOCK_EX); print('locked',flush=True); sys.stdin.read()"]).arg(&lock).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap());
        let mut ready = String::new();
        BufReader::new(worker.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "locked");
        assert!(SessionLease::acquire(&state, "conversation").is_err());
        worker.0.kill().unwrap();
        worker.0.wait().unwrap();
        assert!(SessionLease::acquire(&state, "conversation").is_ok());
        assert!(lock.exists());
    }
    #[test]
    fn plugin_setup_withholds_split_credentials_and_sanitizes_completed_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = CoreHost {
            store: Store::open(&dir.path().join("db")).unwrap(),
            scope: "setup".into(),
            task_id: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            approval_mode: ApprovalMode::Default,
            agent_name: "test".into(),
            plugin_setup: true,
        };
        for delta in ["xoxp-", "private-value"] {
            host.event(Event::Text {
                delta: delta.into(),
            })
            .unwrap();
        }
        host.event(Event::Progress {
            method: "browser".into(),
            data: json!({"screenshot":"private-value","token":"opaque"}),
        })
        .unwrap();
        host.event(Event::Completed {
            output: "xoxp-private-value ghp_private-value".into(),
        })
        .unwrap();
        let events = host.store.scope_events("setup").unwrap();
        assert_eq!(events.len(), 2);
        assert!(!json!(events).to_string().contains("private-value"));
        assert!(!json!(events).to_string().contains("opaque"));
        assert_eq!(events[1]["event"]["output"], "[redacted] [redacted]");
    }

    #[test]
    fn core_persists_sessions_and_relays_approval_without_reexecution() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let mut host = CoreHost {
            store: Store::open(&db).unwrap(),
            scope: "project-a/agent-a".into(),
            task_id: 7,
            cancel: Arc::new(AtomicBool::new(false)),
            approval_mode: ApprovalMode::Default,
            agent_name: "test".into(),
            plugin_setup: false,
        };
        host.event(Event::Session {
            id: "thread-1".into(),
        })
        .unwrap();
        let store = Store::open(&db).unwrap();
        assert_eq!(
            store.agent_session("project-a/agent-a").unwrap().as_deref(),
            Some("thread-1")
        );
        assert!(store.agent_session("project-b/agent-a").unwrap().is_none());
        let worker = std::thread::spawn(move || {
            host.approve(ApprovalRequest {
                id: json!(42),
                method: "item/commandExecution/requestApproval".into(),
                params: json!({"command":"echo hi"}),
            })
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let id = loop {
            if let Some(item) = store.approvals(Some("pending")).unwrap().first() {
                break item.id;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(crate::execute_approval(&store, id).is_ok());
        assert!(crate::execute_approval(&store, id).is_err());
        assert!(matches!(
            worker.join().unwrap().unwrap(),
            ApprovalDecision::Accept
        ));
        assert_eq!(store.approval(id).unwrap().unwrap().status, "expired");
        assert!(!store.events(7).unwrap().is_empty());
    }
    #[test]
    fn read_only_web_requests_bypass_the_user_queue_in_default_and_safe_modes() {
        for mode in [ApprovalMode::Default, ApprovalMode::Safe] {
            let dir = tempfile::tempdir().unwrap();
            let mut host = CoreHost {
                store: Store::open(&dir.path().join("db")).unwrap(),
                scope: "web".into(),
                task_id: 0,
                cancel: Arc::new(AtomicBool::new(false)),
                approval_mode: mode,
                agent_name: "test".into(),
                plugin_setup: false,
            };
            let response = host.begin_request(ApprovalRequest {
                id: json!("read"),
                method: "item/commandExecution/requestApproval".into(),
                params: json!({"command":"curl -q -fsSL --proto-redir '=http,https' https://example.com","availableDecisions":["accept","decline","cancel"]}),
            }).unwrap();
            assert!(
                matches!(response, RequestResponse::Ready(value) if value == json!({"decision":"accept"}))
            );
            assert!(host.store.approvals(None).unwrap().is_empty());
            let events = host.store.scope_events("web").unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["approval"], "auto-approved");
            assert_eq!(events[0]["reason"], "read-only-web");
        }
    }

    #[test]
    fn writes_network_grants_and_unavailable_read_decisions_still_wait_for_a_user() {
        for (method, params) in [
            (
                "item/commandExecution/requestApproval",
                json!({"command":"curl -q -X POST https://example.com"}),
            ),
            (
                "item/commandExecution/requestApproval",
                json!({"command":"curl -q -o file.html https://example.com"}),
            ),
            (
                "item/permissions/requestApproval",
                json!({"permissions":{"network":{"enabled":true}}}),
            ),
            (
                "item/commandExecution/requestApproval",
                json!({"command":"curl -q https://example.com","additionalPermissions":{"fileSystem":{"write":["/tmp"]}}}),
            ),
            (
                "item/commandExecution/requestApproval",
                json!({"command":"curl -q https://example.com","availableDecisions":["decline","cancel"]}),
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("db");
            let cancel = Arc::new(AtomicBool::new(false));
            let mut host = CoreHost {
                store: Store::open(&db).unwrap(),
                scope: "web".into(),
                task_id: 0,
                cancel: cancel.clone(),
                approval_mode: ApprovalMode::Safe,
                agent_name: "test".into(),
                plugin_setup: false,
            };
            let request = ApprovalRequest {
                id: json!("write"),
                method: method.into(),
                params,
            };
            let worker = std::thread::spawn(move || host.begin_request(request));
            let store = Store::open(&db).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            let id = loop {
                if let Some(item) = store.approvals(Some("pending")).unwrap().first() {
                    break item.id;
                }
                if Instant::now() >= deadline {
                    cancel.store(true, Ordering::Relaxed);
                    let _ = worker.join();
                    panic!("expected pending approval for {method}");
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert!(store.resolve_approval(id, "rejected").unwrap());
            let response = worker.join().unwrap().unwrap();
            assert!(matches!(response, RequestResponse::Ready(_)));
            assert_eq!(store.approval(id).unwrap().unwrap().status, "expired");
        }
    }

    #[test]
    fn cancelled_approval_cannot_be_reused() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = CoreHost {
            store: Store::open(&dir.path().join("db")).unwrap(),
            scope: "scope".into(),
            task_id: 0,
            cancel: Arc::new(AtomicBool::new(true)),
            approval_mode: ApprovalMode::Default,
            agent_name: "test".into(),
            plugin_setup: false,
        };
        assert!(host
            .approve(ApprovalRequest {
                id: json!(1),
                method: "file".into(),
                params: json!({})
            })
            .is_err());
        assert!(host.store.approvals(Some("pending")).unwrap().is_empty());
    }
}
