use serde_json::{json, Value};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

async fn send(input: &mut ChildStdin, value: Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    input.write_all(&bytes).await.map_err(|e| e.to_string())?;
    input.flush().await.map_err(|e| e.to_string())
}

async fn call(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    id: u64,
    method: &str,
    params: Value,
    text: &mut String,
    auto_approve: &[String],
) -> Result<Value, String> {
    send(
        input,
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )
    .await?;
    loop {
        let mut line = String::new();
        if output
            .read_line(&mut line)
            .await
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err(format!("ACP 在 {method} 响应前关闭连接"));
        }
        let message: Value =
            serde_json::from_str(&line).map_err(|e| format!("ACP JSON 无效：{e}"))?;
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err("ACP 缺少 JSON-RPC 版本".into());
        }
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if let Some(request_id) = message.get("id") {
                let response = if method == "session/request_permission" {
                    let title = message["params"]["toolCall"]["title"]
                        .as_str()
                        .unwrap_or("");
                    let allowed = auto_approve.iter().any(|rule| title.contains(rule));
                    json!({"jsonrpc":"2.0","id":request_id,"result":{"outcome":{"outcome":if allowed { "selected" } else { "cancelled" }}}})
                } else {
                    json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32601,"message":"Client capability not supported"}})
                };
                send(input, response).await?;
                if method == "session/request_permission"
                    && !auto_approve.iter().any(|rule| {
                        message["params"]["toolCall"]["title"]
                            .as_str()
                            .unwrap_or("")
                            .contains(rule)
                    })
                {
                    return Err("ACP 请求工具权限，需要审批支持后重试".into());
                }
            } else if method == "session/update" {
                let update = &message["params"]["update"];
                if update["sessionUpdate"] == "agent_message_chunk"
                    && update["content"]["type"] == "text"
                {
                    if let Some(chunk) = update["content"]["text"].as_str() {
                        text.push_str(chunk);
                    }
                }
            }
            continue;
        }
        if message.get("id") != Some(&json!(id)) {
            return Err("ACP 响应 ID 不匹配".into());
        }
        if let Some(error) = message.get("error") {
            return Err(format!("ACP {method} 失败，错误码 {}", error["code"]));
        }
        return message
            .get("result")
            .cloned()
            .ok_or_else(|| "ACP 响应缺少 result".into());
    }
}

#[cfg(test)]
pub fn run(
    command: &str,
    args: &[String],
    cwd: &Path,
    prompt: &str,
    cancel: Arc<AtomicBool>,
    auto_approve: &[String],
) -> Result<String, String> {
    run_with_mcp(command, args, cwd, prompt, cancel, auto_approve, &[])
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_mcp(
    command: &str,
    args: &[String],
    cwd: &Path,
    prompt: &str,
    cancel: Arc<AtomicBool>,
    auto_approve: &[String],
    mcp_servers: &[Value],
) -> Result<String, String> {
    let cwd = cwd
        .canonicalize()
        .map_err(|e| format!("项目目录无效：{e}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args).current_dir(&cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        #[cfg(unix)] cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| format!("ACP 启动失败：{e}"))?;
        let pid = child.id();
        let mut input = child.stdin.take().ok_or("ACP stdin unavailable")?;
        let mut output = BufReader::new(child.stdout.take().ok_or("ACP stdout unavailable")?);
        let work = async {
            let mut text = String::new();
            let init = call(&mut input, &mut output, 1, "initialize", json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"omnistream","version":"0.1.0"}}), &mut text, auto_approve).await?;
            if init["protocolVersion"] != 1 { return Err("ACP 协议版本不支持".into()); }
            let session = call(&mut input, &mut output, 2, "session/new", json!({"cwd":cwd,"mcpServers":mcp_servers}), &mut text, auto_approve).await?;
            let session_id = session["sessionId"].as_str().ok_or("ACP 缺少 sessionId")?;
            let result = call(&mut input, &mut output, 3, "session/prompt", json!({"sessionId":session_id,"prompt":[{"type":"text","text":prompt}]}), &mut text, auto_approve).await?;
            if result["stopReason"] != "end_turn" { return Err(format!("ACP 未正常完成：{}",result["stopReason"])); }
            Ok(text)
        };
        let result = tokio::select! {
            result = work => result,
            _ = async { while !cancel.load(Ordering::Relaxed) { tokio::time::sleep(Duration::from_millis(25)).await; } } => Err::<String, String>("ACP 请求已取消".into()),
        };
        #[cfg(unix)] if let Some(pid) = pid { unsafe { libc::kill(-(pid as i32), libc::SIGKILL); } }
        let _ = child.start_kill();
        let _ = child.wait().await;
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_json_rpc_handshake_and_updates() {
        let script = r#"
read request
case "$request" in *'"method":"initialize"'*) ;; *) exit 1;; esac
echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
read request
case "$request" in *'"method":"session/new"'*) ;; *) exit 2;; esac
echo '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s"}}'
read request
case "$request" in *'"method":"session/prompt"'*) ;; *) exit 3;; esac
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}}}}'
echo '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
sleep 10
"#;
        assert_eq!(
            run(
                "sh",
                &["-c".into(), script.into()],
                Path::new("."),
                "hello",
                Arc::new(AtomicBool::new(false)),
                &[]
            )
            .unwrap(),
            "done"
        );
    }
    #[test]
    fn eof_and_cancellation_are_failures() {
        assert!(run(
            "true",
            &[],
            Path::new("."),
            "hello",
            Arc::new(AtomicBool::new(false)),
            &[]
        )
        .is_err());
        let start = std::time::Instant::now();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::Relaxed);
        });
        assert!(run(
            "sleep",
            &["10".into()],
            Path::new("."),
            "hello",
            cancel,
            &[]
        )
        .unwrap_err()
        .contains("取消"));
        worker.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
