//! Codex App Server stdio client (JSON-RPC without the jsonrpc field).
use crate::{
    adapter::{ApprovalRequest, Event, Host, Request, RequestResponse},
    config::{AgentConfig, ApprovalMode, Config},
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

#[derive(Debug)]
enum RpcError {
    Remote { method: String, error: Value },
    Transport(String),
}
impl RpcError {
    fn writer_conflict(&self, thread: &str) -> bool {
        matches!(self, Self::Remote { method, error }
            if method == "thread/resume" && error["code"] == -32600
                && error["message"].as_str() == Some(format!("thread {thread} already has an active writer").as_str()))
    }
}
impl From<String> for RpcError {
    fn from(error: String) -> Self {
        Self::Transport(error)
    }
}
impl From<&str> for RpcError {
    fn from(error: &str) -> Self {
        Self::Transport(error.into())
    }
}
impl From<RpcError> for String {
    fn from(error: RpcError) -> Self {
        match error {
            RpcError::Remote { method, error } => format!("App Server {method}: {error}"),
            RpcError::Transport(error) => error,
        }
    }
}

struct Transport {
    child: Child,
    input: ChildStdin,
    messages: mpsc::Receiver<Result<Value, String>>,
    cancel: Arc<AtomicBool>,
    thread: Option<String>,
    turn: Option<String>,
    pending: std::collections::VecDeque<Value>,
}
impl Drop for Transport {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Transport {
    fn send(&mut self, value: Value) -> Result<(), String> {
        writeln!(self.input, "{value}")
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())
    }
    fn next(&mut self) -> Result<Value, String> {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                if let (Some(thread), Some(turn)) = (&self.thread, &self.turn) {
                    let _ = self.send(json!({"id":99,"method":"turn/interrupt","params":{"threadId":thread,"turnId":turn}}));
                }
                return Err("App Server 已取消".into());
            }
            match self.messages.recv_timeout(Duration::from_millis(25)) {
                Ok(message) => return message,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("App Server 意外关闭输出".into())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
        }
    }
    fn dispatch(&mut self, msg: &Value, host: &mut dyn Host) -> Result<(), String> {
        let Some(method) = msg["method"].as_str() else {
            return Ok(());
        };
        if let Some(id) = msg.get("id") {
            if method == "item/tool/call" {
                let request = ApprovalRequest {
                    id: id.clone(),
                    method: method.into(),
                    params: msg["params"].clone(),
                };
                let response = host.dynamic_tool(request).unwrap_or_else(|error| json!({"success":false,"contentItems":[{"type":"inputText","text":error}]}));
                self.send(json!({"id":id,"result":response}))?;
            } else if crate::server_requests::interactive(method) {
                if self.thread.as_deref() != msg["params"]["threadId"].as_str()
                    || self
                        .turn
                        .as_deref()
                        .is_some_and(|turn| Some(turn) != msg["params"]["turnId"].as_str())
                {
                    self.send(json!({"id":id,"result":{"decision":"cancel"}}))?;
                    return Err("审批请求不属于当前 thread".into());
                }
                let request = ApprovalRequest {
                    id: id.clone(),
                    method: method.into(),
                    params: msg["params"].clone(),
                };
                let response = match host.begin_request(request) {
                    Ok(RequestResponse::Ready(response)) => response,
                    Ok(RequestResponse::Pending(_)) => return Err("异步请求未实现".into()),
                    Err(error) => {
                        let _ = self.send(json!({"id":id,"result":{"decision":"cancel"}}));
                        if let (Some(thread), Some(turn)) = (&self.thread, &self.turn) {
                            let _ = self.send(json!({"id":99,"method":"turn/interrupt","params":{"threadId":thread,"turnId":turn}}));
                        }
                        return Err(error);
                    }
                };
                self.send(json!({"id":id,"result":response}))?;
            } else if method == "account/chatgptAuthTokens/refresh" {
                self.send(json!({"id":id,"error":{"code":-32000,"message":"OmniStream does not own external ChatGPT tokens"}}))?;
            } else if method == "attestation/generate" {
                self.send(json!({"id":id,"error":{"code":-32000,"message":"OmniStream has no upstream attestation provider"}}))?;
            } else {
                self.send(json!({"id":id,"error":{"code":-32601,"message":"OmniStream does not support this server request"}}))?;
            }
        } else if msg["params"]["threadId"]
            .as_str()
            .is_some_and(|id| self.thread.as_deref().is_some_and(|thread| id != thread))
            || msg["params"]["turnId"]
                .as_str()
                .is_some_and(|id| self.turn.as_deref().is_some_and(|turn| id != turn))
        {
            return Ok(());
        } else if method == "item/agentMessage/delta" {
            if let Some(delta) = msg["params"]["delta"].as_str() {
                host.event(Event::Text {
                    delta: delta.into(),
                })?;
            }
        } else {
            host.event(Event::Progress {
                method: method.into(),
                data: msg["params"].clone(),
            })?;
        }
        Ok(())
    }
    fn call(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        host: &mut dyn Host,
    ) -> Result<Value, RpcError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err("App Server 已取消".into());
        }
        self.send(json!({"id":id,"method":method,"params":params}))?;
        loop {
            let msg = self.next()?;
            if method == "turn/start" && msg.get("method").is_some() && msg.get("id").is_none() {
                self.pending.push_back(msg);
                continue;
            }
            if msg.get("method").is_some() {
                self.dispatch(&msg, host)?;
                continue;
            }
            if msg["id"] != id {
                return Err("App Server 响应 ID 不匹配".into());
            }
            if let Some(error) = msg.get("error") {
                return Err(RpcError::Remote {
                    method: method.into(),
                    error: error.clone(),
                });
            }
            return msg
                .get("result")
                .cloned()
                .ok_or("App Server 缺少 result".into());
        }
    }
}

pub fn run(
    agent: &AgentConfig,
    config: &Config,
    request: Request<'_>,
    host: &mut dyn Host,
    cancel: Arc<AtomicBool>,
) -> Result<String, String> {
    let cwd = request.cwd.canonicalize().map_err(|e| e.to_string())?;
    let mut command = Command::new(&agent.command);
    command.args(agent.app_server_args()?);
    if let Some(model) = &agent.inherited_model {
        model.configure(&mut command)?;
    }
    command
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动 App Server 失败：{e}"))?;
    let input = child.stdin.take().ok_or("stdin unavailable")?;
    let output = child.stdout.take().ok_or("stdout unavailable")?;
    let (tx, messages) = mpsc::sync_channel(256);
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let message = line.map_err(|e| e.to_string()).and_then(|l| {
                serde_json::from_str(&l).map_err(|e| format!("App Server JSON 无效：{e}"))
            });
            if tx.send(message).is_err() {
                break;
            }
        }
    });
    let mut rpc = Transport {
        child,
        input,
        messages,
        cancel,
        thread: None,
        turn: None,
        pending: std::collections::VecDeque::new(),
    };
    rpc.call(1, "initialize", json!({"clientInfo":{"name":"omnistream","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false}}), host)?;
    rpc.send(json!({"method":"initialized","params":{}}))?;
    let mut params = json!({"cwd":cwd});
    if let Some(model) = &agent.inherited_model {
        model.thread_params(&mut params)?;
    }
    match config.approval_mode() {
        ApprovalMode::Default => {}
        ApprovalMode::Safe => {
            params["approvalPolicy"] = json!("on-request");
            params["sandbox"] = json!("workspace-write");
        }
        ApprovalMode::All => {
            params["approvalPolicy"] = json!("never");
            params["sandbox"] = json!("danger-full-access");
        }
    }
    let method = if let Some(id) = &request.session {
        params["threadId"] = json!(id);
        "thread/resume"
    } else {
        "thread/start"
    };
    let (result, forked) = match rpc.call(2, method, params.clone(), host) {
        Ok(result) => (result, false),
        Err(error)
            if request
                .session
                .as_deref()
                .is_some_and(|id| error.writer_conflict(id)) =>
        {
            // A resume rejection precedes all work for this request. Fork once; never
            // replay a turn or replace history with a blank thread on other errors.
            params["excludeTurns"] = json!(true);
            (rpc.call(3, "thread/fork", params, host)?, true)
        }
        Err(error) => return Err(error.into()),
    };
    let thread = result["thread"]["id"]
        .as_str()
        .filter(|id| !id.trim().is_empty())
        .ok_or("App Server 缺少 thread.id")?
        .to_string();
    if forked && request.session.as_deref() == Some(thread.as_str()) {
        return Err("App Server 分支未创建独立线程，停止执行".into());
    }
    rpc.thread = Some(thread.clone());
    host.event(Event::Session { id: thread.clone() })?;
    if forked {
        host.event(Event::Progress {
            method: "omnistream/thread/forked".into(),
            data: json!({"source_thread_id":request.session,"thread_id":thread,"reason":"active_writer","message":"原线程被其他入口占用，已创建保留历史的独立分支；后续对话使用新线程。"}),
        })?;
    }
    let mut turn_params =
        json!({"threadId":thread,"input":[{"type":"text","text":request.prompt}]});
    if let Some(model) = &agent.inherited_model {
        turn_params["model"] = json!(model.model()?);
        turn_params["effort"] = json!(model.effort());
    }
    let result = rpc.call(if forked { 4 } else { 3 }, "turn/start", turn_params, host)?;
    let turn = result["turn"]["id"]
        .as_str()
        .ok_or("App Server 缺少 turn.id")?
        .to_string();
    rpc.turn = Some(turn.clone());
    let mut output = String::new();
    loop {
        let msg = match rpc.pending.pop_front() {
            Some(msg) => msg,
            None => rpc.next()?,
        };
        rpc.dispatch(&msg, host)?;
        if msg["params"]["threadId"]
            .as_str()
            .is_some_and(|id| id != thread)
        {
            continue;
        }
        if msg["method"] == "item/completed" && msg["params"]["item"]["type"] == "agentMessage" {
            output = msg["params"]["item"]["text"]
                .as_str()
                .unwrap_or_default()
                .into();
        }
        if msg["method"] == "turn/completed" && msg["params"]["turn"]["id"] == turn {
            if msg["params"]["turn"]["status"] != "completed" {
                return Err(format!("App Server turn 未完成：{}", msg["params"]["turn"]));
            }
            host.event(Event::Completed {
                output: output.clone(),
            })?;
            return Ok(output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{tests::RecordingHost, ApprovalDecision};
    fn agent(script: &str) -> AgentConfig {
        AgentConfig {
            inherited_model: None,
            plugin_transport: Default::default(),
            mcp_args: Vec::new(),
            plugin_tools: Default::default(),
            config_file: None,
            command: "sh".into(),
            args: vec!["-c".into(), script.into()],
            protocol: Some("app-server".into()),
            timeout_ms: Some(2000),
            retries: None,
        }
    }
    #[test]
    fn protocol_resume_streaming_and_bidirectional_approval() {
        let script = r#"
read init
case "$init" in *'"method":"initialize"'*) ;; *) exit 1;; esac
echo '{"id":1,"result":{}}'
read initialized
case "$initialized" in *'"method":"initialized"'*) ;; *) exit 2;; esac
read resume
case "$resume" in *'"method":"thread/resume"'*'"threadId":"saved"'*) ;; *) exit 3;; esac
echo '{"id":2,"result":{"thread":{"id":"saved"}}}'
read turn
echo '{"id":"approval1","method":"item/commandExecution/requestApproval","params":{"threadId":"saved","turnId":"t","command":"test"}}'
read answer
case "$answer" in *'"decision":"decline"'*) ;; *) exit 4;; esac
echo '{"id":88,"method":"item/fileChange/requestApproval","params":{"threadId":"saved","turnId":"t"}}'
read answer
case "$answer" in *'"decision":"decline"'*) ;; *) exit 5;; esac
echo '{"id":89,"method":"unknown/request","params":{}}'
read answer
case "$answer" in *'-32601'*) ;; *) exit 6;; esac
echo '{"method":"item/agentMessage/delta","params":{"threadId":"saved","turnId":"t","delta":"hello"}}'
echo '{"method":"item/completed","params":{"threadId":"saved","turnId":"t","item":{"type":"agentMessage","text":"hello"}}}'
echo '{"method":"turn/completed","params":{"threadId":"saved","turn":{"id":"t","status":"completed"}}}'
echo '{"id":3,"result":{"turn":{"id":"t"}}}'
"#;
        let mut host = RecordingHost {
            decision: ApprovalDecision::Decline,
            ..Default::default()
        };
        let result = run(
            &agent(script),
            &Config::default(),
            Request {
                cwd: std::path::Path::new("."),
                prompt: "hello",
                session: Some("saved".into()),
            },
            &mut host,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(result, "hello");
        assert_eq!(host.approvals.len(), 2);
        assert!(host
            .events
            .iter()
            .any(|e| matches!(e, Event::Text {delta} if delta == "hello")));
    }
    #[test]
    fn rejects_rpc_errors_eof_invalid_json_and_cancellation() {
        for script in [
            "exit 0",
            "echo broken",
            "echo '{\"id\":1,\"error\":{\"code\":-1,\"message\":\"bad\"}}'",
        ] {
            assert!(run(
                &agent(script),
                &Config::default(),
                Request {
                    cwd: std::path::Path::new("."),
                    prompt: "x",
                    session: None
                },
                &mut RecordingHost::default(),
                Arc::new(AtomicBool::new(false))
            )
            .is_err());
        }
        assert!(run(
            &agent("sleep 10"),
            &Config::default(),
            Request {
                cwd: std::path::Path::new("."),
                prompt: "x",
                session: None
            },
            &mut RecordingHost::default(),
            Arc::new(AtomicBool::new(true))
        )
        .unwrap_err()
        .contains("取消"));
    }

    fn recorded_calls(directory: &std::path::Path) -> Vec<Value> {
        std::fs::read_to_string(directory.join("calls.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    const RESUME_FIXTURE: &str = r#"
read init
echo '{"id":1,"result":{}}'
read initialized
read resume
printf '%s\n' "$resume" >> calls.jsonl
"#;
    const BUSY: &str = r#"echo '{"id":2,"error":{"code":-32600,"message":"thread saved already has an active writer"}}'"#;

    #[test]
    fn occupied_resume_forks_once_preserves_policy_and_handles_early_turn_events() {
        let directory = tempfile::tempdir().unwrap();
        let script = format!(
            r#"{RESUME_FIXTURE}
{BUSY}
read fork
printf '%s\n' "$fork" >> calls.jsonl
echo '{{"id":3,"result":{{"thread":{{"id":"branch"}}}}}}'
read turn
printf '%s\n' "$turn" >> calls.jsonl
echo '{{"id":"approval","method":"item/commandExecution/requestApproval","params":{{"threadId":"branch","turnId":"work","command":"inspect"}}}}'
read approval
echo '{{"method":"item/completed","params":{{"threadId":"branch","turnId":"work","item":{{"type":"agentMessage","text":"kept history"}}}}}}'
echo '{{"method":"turn/completed","params":{{"threadId":"branch","turn":{{"id":"work","status":"completed"}}}}}}'
echo '{{"id":4,"result":{{"turn":{{"id":"work"}}}}}}'
"#
        );
        let mut host = RecordingHost::default();
        let mut config = Config::default();
        config.approval_mode = Some(ApprovalMode::Safe);
        let output = run(
            &agent(&script),
            &config,
            Request {
                cwd: directory.path(),
                prompt: "continue",
                session: Some("saved".into()),
            },
            &mut host,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(output, "kept history");
        let calls = recorded_calls(directory.path());
        assert_eq!(
            calls
                .iter()
                .map(|c| c["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["thread/resume", "thread/fork", "turn/start"]
        );
        let mut expected = calls[0]["params"].clone();
        expected["excludeTurns"] = json!(true);
        assert_eq!(calls[1]["params"], expected);
        assert_eq!(calls[1]["params"]["approvalPolicy"], "on-request");
        assert_eq!(calls[1]["params"]["sandbox"], "workspace-write");
        assert_eq!(calls[2]["params"]["threadId"], "branch");
        assert_eq!(host.approvals.len(), 1);
        assert!(matches!(&host.events[0], Event::Session { id } if id == "branch"));
        assert!(host.events.iter().any(|e| matches!(e, Event::Progress { method, data } if method == "omnistream/thread/forked" && data["source_thread_id"] == "saved" && data["thread_id"] == "branch")));
    }

    #[test]
    fn only_the_saved_threads_explicit_writer_error_can_trigger_a_fork() {
        for error in [
            json!({"code":-32600,"message":"invalid credentials"}),
            json!({"code":-1,"message":"thread saved already has an active writer"}),
            json!({"code":-32600,"message":"thread other already has an active writer"}),
            json!({"code":-32600,"message":"thread not found"}),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let reply = json!({"id":2,"error":error});
            let script = format!(
                r#"{RESUME_FIXTURE}
echo '{reply}'
while read unexpected; do printf '%s\n' "$unexpected" >> calls.jsonl; echo '{{"id":3,"error":{{"code":-1,"message":"unexpected replay"}}}}'; done
"#
            );
            let result = run(
                &agent(&script),
                &Config::default(),
                Request {
                    cwd: directory.path(),
                    prompt: "work",
                    session: Some("saved".into()),
                },
                &mut RecordingHost::default(),
                Arc::new(AtomicBool::new(false)),
            );
            assert!(result.is_err());
            assert_eq!(recorded_calls(directory.path()).len(), 1, "{error}");
        }
    }

    #[test]
    fn fork_failure_or_invalid_thread_id_never_starts_work_or_discards_the_parent() {
        for reply in [
            json!({"id":3,"error":{"code":-1,"message":"fork unavailable"}}),
            json!({"id":3,"result":{"thread":{"id":"saved"}}}),
            json!({"id":3,"result":{"thread":{"id":" "}}}),
            json!({"id":3,"result":{}}),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let script = format!(
                r#"{RESUME_FIXTURE}
{BUSY}
read fork
printf '%s\n' "$fork" >> calls.jsonl
echo '{reply}'
while read unexpected; do printf '%s\n' "$unexpected" >> calls.jsonl; echo '{{"id":4,"error":{{"code":-1,"message":"unexpected replay"}}}}'; done
"#
            );
            let mut host = RecordingHost::default();
            assert!(run(
                &agent(&script),
                &Config::default(),
                Request {
                    cwd: directory.path(),
                    prompt: "work",
                    session: Some("saved".into())
                },
                &mut host,
                Arc::new(AtomicBool::new(false))
            )
            .is_err());
            assert_eq!(recorded_calls(directory.path()).len(), 2);
            assert!(!host
                .events
                .iter()
                .any(|e| matches!(e, Event::Session { .. })));
        }
    }

    #[test]
    fn writer_error_after_turn_start_is_reported_without_forking_or_replaying() {
        let directory = tempfile::tempdir().unwrap();
        let script = format!(
            r#"{RESUME_FIXTURE}
echo '{{"id":2,"result":{{"thread":{{"id":"saved"}}}}}}'
read turn
printf '%s\n' "$turn" >> calls.jsonl
echo '{{"id":3,"error":{{"code":-32600,"message":"thread saved already has an active writer"}}}}'
while read unexpected; do printf '%s\n' "$unexpected" >> calls.jsonl; echo '{{"id":4,"error":{{"code":-1,"message":"unexpected replay"}}}}'; done
"#
        );
        let error = run(
            &agent(&script),
            &Config::default(),
            Request {
                cwd: directory.path(),
                prompt: "work",
                session: Some("saved".into()),
            },
            &mut RecordingHost::default(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap_err();
        assert!(error.contains("turn/start"));
        let calls = recorded_calls(directory.path());
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1]["method"], "turn/start");
    }

    #[test]
    #[ignore = "requires Codex CLI; uses only a loopback mock model"]
    fn real_codex_with_mock_model_and_resume() {
        use crate::mock_model::{MockServer, Step};
        let reply = |text: &str| {
            let item = json!({"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]});
            Step {
                status: 200,
                delay_ms: 0,
                disconnect: false,
                body: Value::Null,
                expect_bearer: Some("inherited-fixture-key".into()),
                events: vec![
                    json!({"type":"response.created","response":{"id":"resp_1","status":"in_progress","output":[]}}),
                    json!({"type":"response.output_item.added","output_index":0,"item":item}),
                    json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":text}),
                    json!({"type":"response.output_item.done","output_index":0,"item":item}),
                    json!({"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[item],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}),
                ],
            }
        };
        let server = MockServer::start(
            "127.0.0.1:0",
            vec![
                reply("first answer"),
                reply("second answer"),
                reply("forked answer"),
                reply("branch answer"),
            ],
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let codex = std::env::var("OMNISTREAM_TEST_CODEX").unwrap_or_else(|_| "codex".into());
        let args = vec![
            format!("CODEX_HOME={}", dir.path().display()),
            "CODEX_API_KEY=mock".into(),
            codex,
            "app-server".into(),
        ];
        let settings = [
            "model_provider=\"mock\"".to_string(),
            "model=\"native-model-must-not-be-used\"".into(),
            "model_providers.mock.name=\"Mock\"".into(),
            format!("model_providers.mock.base_url=\"{}/v1\"", server.url),
            "model_providers.mock.wire_api=\"responses\"".into(),
            "model_providers.mock.requires_openai_auth=false".into(),
            "model_providers.mock.supports_websockets=false".into(),
            "model_providers.mock.request_max_retries=0".into(),
            "model_providers.mock.stream_max_retries=0".into(),
        ];
        let selected: Config = toml::from_str(&format!(
            r#"
model_provider = "selected"
model = "mock"
model_reasoning_effort = "low"
[model_providers.selected]
base_url = "{}/v1"
wire_api = "responses"
[model_providers.selected.auth]
command = "printf"
args = ["inherited-fixture-key"]
"#,
            server.url
        ))
        .unwrap();
        let config_file = dir.path().join("mock.config.toml");
        std::fs::write(&config_file, settings.join("\n")).unwrap();
        let agent = AgentConfig {
            inherited_model: crate::delegated_model::SelectedModel::capture(&selected),
            plugin_transport: Default::default(),
            mcp_args: Vec::new(),
            plugin_tools: Default::default(),
            config_file: Some(config_file),
            command: "env".into(),
            args,
            protocol: Some("app-server".into()),
            timeout_ms: Some(15000),
            retries: None,
        };
        let mut host = RecordingHost::default();
        let output = run(
            &agent,
            &Config::default(),
            Request {
                cwd: dir.path(),
                prompt: "hello",
                session: None,
            },
            &mut host,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(output, "first answer");
        let session = host
            .events
            .iter()
            .find_map(|event| {
                if let Event::Session { id } = event {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .unwrap();
        let output = run(
            &agent,
            &Config::default(),
            Request {
                cwd: dir.path(),
                prompt: "continue",
                session: Some(session.clone()),
            },
            &mut host,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(output, "second answer");
        // Hold the same OS writer lock as a separate terminal. The original
        // thread remains occupied throughout both the fork and its follow-up.
        let locks = dir.path().join("thread-writer-locks");
        std::fs::create_dir_all(&locks).unwrap();
        let writer = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(locks.join(format!("{session}.lock")))
            .unwrap();
        writer.lock().unwrap();
        let output = run(
            &agent,
            &Config::default(),
            Request {
                cwd: dir.path(),
                prompt: "continue in your own thread",
                session: Some(session.clone()),
            },
            &mut host,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(output, "forked answer");
        let branch = host
            .events
            .iter()
            .rev()
            .find_map(|event| match event {
                Event::Session { id } => Some(id.clone()),
                _ => None,
            })
            .unwrap();
        assert_ne!(branch, session);
        assert!(host.events.iter().any(|event| matches!(event, Event::Progress { method, data } if method == "omnistream/thread/forked" && data["source_thread_id"] == session && data["thread_id"] == branch)));
        let output = run(
            &agent,
            &Config::default(),
            Request {
                cwd: dir.path(),
                prompt: "continue the independent branch",
                session: Some(branch),
            },
            &mut host,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(output, "branch answer");
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests.iter().all(|r| r["body"]["model"] == "mock"));
        assert_eq!(requests[0]["body"]["reasoning"]["effort"], "low");
        assert!(requests[1]["body"].to_string().contains("first answer"));
        assert!(requests[2]["body"].to_string().contains("second answer"));
        assert!(requests[3]["body"].to_string().contains("forked answer"));
    }
}
