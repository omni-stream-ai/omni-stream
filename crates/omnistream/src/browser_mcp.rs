//! Opt-in Chrome MCP client. Only the allowlisted tools are exposed through the plugin gateway.
use crate::plugin::Registry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub const ID: &str = "chrome-devtools";
pub const VERSION: &str = "1.12.0";
const TIMEOUT: Duration = Duration::from_secs(60);
const TOOLS: &[&str] = &[
    "click",
    "fill",
    "fill_form",
    "handle_dialog",
    "list_pages",
    "navigate_page",
    "new_page",
    "press_key",
    "select_page",
    "wait_for",
    "take_snapshot",
    "close_page",
];

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub enabled: bool,
    pub verified_at: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Serialize)]
pub struct Summary {
    pub enabled: bool,
    pub status: &'static str,
    pub package_version: &'static str,
    pub verified_at: Option<u64>,
    pub last_error: Option<String>,
}

fn path(registry: &Registry) -> PathBuf {
    registry.root().join("mcp").join(format!("{ID}.json"))
}
pub fn load(registry: &Registry) -> Result<State, String> {
    match fs::read(path(registry)) {
        Ok(data) => serde_json::from_slice(&data)
            .map_err(|_| "Chrome 浏览器配置损坏，请重新保存启用设置".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(_) => Err("无法读取 Chrome 浏览器配置".into()),
    }
}
pub fn save(registry: &Registry, state: &State) -> Result<(), String> {
    let path = path(registry);
    fs::create_dir_all(path.parent().unwrap()).map_err(|_| "无法创建浏览器配置目录")?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| -> Result<(), String> {
        let mut file = fs::File::create(&temporary).map_err(|_| "无法创建浏览器配置文件")?;
        file.write_all(&serde_json::to_vec(state).map_err(|_| "无法保存浏览器设置")?)
            .and_then(|_| file.sync_all())
            .map_err(|_| "无法保存浏览器设置")?;
        fs::rename(&temporary, &path).map_err(|_| "无法替换浏览器设置".into())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub fn remove(registry: &Registry) -> Result<(), String> {
    match fs::remove_file(path(registry)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法删除浏览器设置".into()),
    }
}
pub fn summary(registry: &Registry) -> Summary {
    match load(registry) {
        Ok(state) => Summary {
            status: if !state.enabled {
                "disabled"
            } else if state.last_error.is_some() {
                "error"
            } else if state.verified_at.is_some() {
                "ready"
            } else {
                "unverified"
            },
            enabled: state.enabled,
            package_version: VERSION,
            verified_at: state.verified_at,
            last_error: state.last_error,
        },
        Err(error) => Summary {
            status: "error",
            enabled: false,
            package_version: VERSION,
            verified_at: None,
            last_error: Some(error),
        },
    }
}

#[derive(Serialize)]
struct Server {
    command: String,
    args: Vec<String>,
    enabled_tools: Vec<String>,
    startup_timeout_sec: u64,
    tool_timeout_sec: u64,
    required: bool,
}
fn server() -> Server {
    let mut server = Server {
        command: if cfg!(windows) { "npx.cmd" } else { "npx" }.into(),
        args: vec![
            "--yes".into(),
            format!("chrome-devtools-mcp@{VERSION}"),
            "--autoConnect".into(),
            "--no-usage-statistics".into(),
            "--no-performance-crux".into(),
            "--no-javascript-evaluation".into(),
            "--categoryNetwork=false".into(),
            "--categoryPerformance=false".into(),
            "--categoryMemory=false".into(),
            "--categoryEmulation=false".into(),
        ],
        enabled_tools: TOOLS.iter().map(|s| s.to_string()).collect(),
        startup_timeout_sec: TIMEOUT.as_secs(),
        tool_timeout_sec: TIMEOUT.as_secs(),
        required: true,
    };
    // Isolated integration fixtures may replace the executable in debug builds only.
    if cfg!(debug_assertions) {
        if let Some(path) = std::env::var_os("OMNISTREAM_BROWSER_MCP_TEST_COMMAND")
            .filter(|path| std::path::Path::new(path).is_absolute())
        {
            server.command = path.to_string_lossy().into_owned();
            server.args.clear();
        }
    }
    server
}

struct Transport {
    child: Child,
    input: ChildStdin,
    messages: mpsc::Receiver<Result<Value, String>>,
    deadline: Instant,
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
    fn start(server: &Server, timeout: Duration) -> Result<Self, String> {
        let mut command = Command::new(&server.command);
        command
            .args(&server.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|_| "无法启动 Chrome MCP，请在 Core 所在电脑安装受支持的 Node.js 和 npx，并检查 npm 网络连接".to_string())?;
        let input = child.stdin.take().ok_or("无法打开 MCP 输入")?;
        let output = child.stdout.take().ok_or("无法打开 MCP 输出")?;
        let error = child.stderr.take().ok_or("无法打开 MCP 错误流")?;
        let (sender, messages) = mpsc::sync_channel(8);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                match reader
                    .by_ref()
                    .take(4 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) => break,
                    Ok(_) if line.len() <= 4 * 1024 * 1024 => {
                        let value = serde_json::from_slice(&line)
                            .map_err(|_| "Chrome MCP 返回了无效协议数据".into());
                        if sender.send(value).is_err() {
                            break;
                        }
                    }
                    _ => {
                        let _ = sender.send(Err("Chrome MCP 响应过大或无法读取".into()));
                        break;
                    }
                }
            }
        });
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut BufReader::new(error), &mut std::io::sink());
        });
        Ok(Self {
            child,
            input,
            messages,
            deadline: Instant::now() + timeout,
        })
    }
    fn send(&mut self, message: Value) -> Result<(), String> {
        writeln!(self.input, "{message}")
            .and_then(|_| self.input.flush())
            .map_err(|_| "Chrome MCP 连接已关闭".into())
    }
    fn call(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        self.call_controlled(id, method, params, &|| false)
    }
    fn call_controlled(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        if cancelled() {
            return Err("浏览器操作已取消".into());
        }
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        loop {
            if cancelled() {
                return Err("浏览器操作已取消".into());
            }
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Chrome MCP 检查超时；请打开 Chrome 144+，在 chrome://inspect/#remote-debugging 开启远程调试并允许连接，然后重试".into());
            }
            let message = match self
                .messages
                .recv_timeout(remaining.min(Duration::from_millis(50)))
            {
                Ok(message) => message?,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => {
                    return Err(
                        "Chrome MCP 进程已退出，请检查 Node.js / npx 版本和 npm 网络连接".into(),
                    )
                }
            };
            if message.get("method").is_some() {
                if let Some(id) = message.get("id") {
                    self.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported client method"}}))?;
                }
                continue;
            }
            if message["id"] != id {
                return Err("Chrome MCP 响应标识不匹配".into());
            }
            if message.get("error").is_some() {
                return Err("Chrome MCP 请求失败，请检查浏览器远程调试设置并允许连接".into());
            }
            return message
                .get("result")
                .cloned()
                .ok_or("Chrome MCP 响应缺少结果".into());
        }
    }
}

pub struct Session {
    transport: Transport,
    tools: Vec<Value>,
    next_id: u64,
}
impl Session {
    pub fn start(cancelled: &dyn Fn() -> bool) -> Result<Self, String> {
        let mut transport = Transport::start(&server(), TIMEOUT)?;
        let initialized = transport.call_controlled(1, "initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"omnistream","version":env!("CARGO_PKG_VERSION")}}), cancelled)?;
        if initialized["capabilities"]["tools"].as_object().is_none() {
            return Err("Chrome MCP 没有声明浏览器工具能力".into());
        }
        transport.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        let result = transport.call_controlled(2, "tools/list", json!({}), cancelled)?;
        let tools: Vec<_> = result["tools"]
            .as_array()
            .ok_or("Chrome MCP 未返回工具列表")?
            .iter()
            .filter(|tool| {
                tool["name"]
                    .as_str()
                    .is_some_and(|name| TOOLS.contains(&name))
            })
            .cloned()
            .collect();
        if TOOLS
            .iter()
            .any(|name| !tools.iter().any(|tool| tool["name"] == *name))
        {
            return Err("Chrome MCP 缺少所需浏览器工具，请使用兼容版本".into());
        }
        Ok(Self {
            transport,
            tools,
            next_id: 3,
        })
    }
    pub fn tools(&self) -> &[Value] {
        &self.tools
    }
    pub fn call(
        &mut self,
        name: &str,
        args: &Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool["name"] == name)
            .ok_or("此浏览器工具未开放")?;
        if !jsonschema::is_valid(&tool["inputSchema"], args) {
            return Err("浏览器工具参数不符合 inputSchema".into());
        }
        self.transport.deadline = Instant::now() + TIMEOUT;
        self.next_id += 1;
        let result = self.transport.call_controlled(
            self.next_id,
            "tools/call",
            json!({"name":name,"arguments":args}),
            cancelled,
        );
        // A timed-out or cancelled response cannot be matched to a later call.
        if result.is_err() {
            self.transport.deadline = Instant::now();
        }
        result
    }
}

// The probe never returns page titles, URLs, snapshots, or raw subprocess output.
pub fn probe(browser: bool) -> Result<(), String> {
    probe_server(&server(), browser, TIMEOUT)
}
fn probe_server(server: &Server, browser: bool, timeout: Duration) -> Result<(), String> {
    let mut transport = Transport::start(server, timeout)?;
    let initialize = transport.call(1, "initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"omnistream","version":env!("CARGO_PKG_VERSION")}}))?;
    if initialize["capabilities"]["tools"].as_object().is_none() {
        return Err("Chrome MCP 没有声明浏览器工具能力".into());
    }
    transport.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
    let result = transport.call(2, "tools/list", json!({}))?;
    let tools = result["tools"]
        .as_array()
        .ok_or("Chrome MCP 未返回工具列表")?;
    if TOOLS
        .iter()
        .any(|name| !tools.iter().any(|tool| tool["name"] == *name))
    {
        return Err("Chrome MCP 缺少所需浏览器工具，请使用兼容版本".into());
    }
    if browser {
        let result =
            transport.call(3, "tools/call", json!({"name":"list_pages","arguments":{}}))?;
        if result["isError"] == true || !result["content"].is_array() {
            return Err("尚未连接 Chrome；请在 Core 所在电脑打开 Chrome 144+，在 chrome://inspect/#remote-debugging 开启远程调试，并在浏览器中允许连接".into());
        }
    }
    Ok(())
}

pub fn setup_hint() -> &'static str {
    "已通过 OmniStream 通用插件网关接入 Chrome 浏览器。必须先使用 chrome_list_pages 检查连接；原生 MCP 不可用时，使用本轮提供的 plugin-tool 命令，不要仅凭历史回复说没有浏览器工具。若连接失败，指导用户在运行 Core 的电脑上打开 Chrome 144+，在 chrome://inspect/#remote-debugging 开启远程调试并允许连接；不要自动批准浏览器授权。只操作当前插件官方配置所需的页面，不访问无关标签页内容。登录、验证码、授权确认、生成和显示凭据的步骤必须交给用户，避免工具自动返回凭据页面；不要读取令牌、Cookie 或浏览器存储，也不要截图凭据。"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_state_is_opt_in_atomic_and_recovers_after_removal() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::new(dir.path());
        assert!(!load(&registry).unwrap().enabled);
        assert_eq!(summary(&registry).status, "disabled");
        save(
            &registry,
            &State {
                enabled: true,
                verified_at: Some(123),
                last_error: None,
            },
        )
        .unwrap();
        assert_eq!(summary(&registry).status, "ready");
        fs::write(path(&registry), "bad json").unwrap();
        assert_eq!(summary(&registry).status, "error");
        assert!(load(&registry).is_err());
        remove(&registry).unwrap();
        remove(&registry).unwrap();
        assert!(!load(&registry).unwrap().enabled);
    }

    fn fixture(mode: &str) -> Server {
        let script = format!(
            r#"
import json,sys,time
names=json.loads('{}')
for line in sys.stdin:
    request=json.loads(line)
    method=request['method']
    if 'id' not in request: continue
    if method=='initialize': result={{'capabilities':{{'tools':{{}}}}}}
    elif method=='tools/list':
        if sys.argv[1]=='timeout': time.sleep(10)
        if sys.argv[1]=='eof': break
        if sys.argv[1]=='malformed': print('not json',flush=True);continue
        result={{'tools':[{{'name':name}} for name in names if sys.argv[1]!='missing' or name!='take_snapshot']}}
    else: result={{'isError':sys.argv[1]=='disconnected','content':[{{'type':'text','text':'do-not-expose-page-content'}}]}}
    print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':result}}),flush=True)
"#,
            serde_json::to_string(TOOLS).unwrap()
        );
        Server {
            command: "python3".into(),
            args: vec!["-c".into(), script, mode.into()],
            enabled_tools: vec![],
            startup_timeout_sec: 1,
            tool_timeout_sec: 1,
            required: true,
        }
    }

    #[test]
    fn browser_probe_checks_real_protocol_tools_and_connection_without_returning_page_content() {
        assert!(probe_server(&fixture("ok"), true, Duration::from_secs(2)).is_ok());
        assert!(probe_server(&fixture("disconnected"), false, Duration::from_secs(2)).is_ok());
        for mode in ["disconnected", "missing", "eof", "malformed", "timeout"] {
            let error = probe_server(&fixture(mode), true, Duration::from_millis(150)).unwrap_err();
            assert!(!error.contains("do-not-expose"));
        }
    }

    #[test]
    #[ignore = "downloads the pinned public npm package; does not connect to a browser or model"]
    fn browser_official_mcp_exposes_the_configured_tools() {
        probe(false).unwrap();
    }
}
