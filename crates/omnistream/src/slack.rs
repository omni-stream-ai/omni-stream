//! Slack Socket Mode receiver. It never calls the model or sends a chat reply.
use crate::{
    plugin_connections::{AuthMode, Connection, Summary},
    plugin_connectors::Connector,
    slack_inbox::Inbox,
};
use reqwest::Url;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    net::{TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

#[derive(Clone, serde::Serialize)]
struct Status {
    state: &'static str,
    error: Option<String>,
    connected_at: Option<u64>,
    last_event_at: Option<u64>,
    #[serde(skip)]
    generation: String,
}
static STATUS: Mutex<Status> = Mutex::new(Status {
    state: "stopped",
    error: None,
    connected_at: None,
    last_event_at: None,
    generation: String::new(),
});
static RECONNECT: AtomicU64 = AtomicU64::new(0);

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn active(c: &Connection) -> bool {
    c.enabled
        && c.listen
        && c.auth_mode == AuthMode::SlackUser
        && c.app_token.is_some()
        && c.user_id.is_some()
        && c.team_id.is_some()
}
fn current(c: &Connection, epoch: u64) -> bool {
    epoch == RECONNECT.load(Ordering::Relaxed)
        && crate::plugin_marketplace::slack_connection()
            .ok()
            .flatten()
            .is_some_and(|latest| latest.generation == c.generation && active(&latest))
}
fn update(c: &Connection, state: &'static str, error: Option<String>) {
    if let Ok(mut status) = STATUS.lock() {
        if status.generation != c.generation {
            status.last_event_at = None;
            status.connected_at = None;
        }
        status.generation = c.generation.clone();
        status.state = state;
        status.error = error;
        if state == "listening" {
            status.connected_at = Some(now());
        }
    }
}

pub fn start() -> Result<(), String> {
    std::thread::Builder::new()
        .name("omnistream-slack".into())
        .spawn(|| loop {
            let connection = crate::plugin_marketplace::slack_connection().ok().flatten();
            let Some(c) = connection.filter(active) else {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            };
            let epoch = RECONNECT.load(Ordering::Relaxed);
            let mut delay = 1;
            while current(&c, epoch) {
                update(&c, "connecting", None);
                let result = (|| {
                    let connector = Connector::new("slack", c.app_token.as_deref().unwrap())
                        .map_err(|e| (e, true))?;
                    let url = connector.open_socket().map_err(|e| {
                        let terminal = e.auth_failed
                            || e.message.contains("missing_scope")
                            || e.message.contains("not_allowed_token_type");
                        (e.message, terminal)
                    })?;
                    if !current(&c, epoch) {
                        return Ok(());
                    }
                    receive(&c, epoch, url).map_err(|e| (e, false))
                })();
                if !current(&c, epoch) {
                    break;
                }
                let (error, terminal) = result
                    .err()
                    .unwrap_or(("Slack 连接已关闭，正在重新连接".into(), false));
                if terminal {
                    update(&c, "error", Some(error));
                    while current(&c, epoch) {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    break;
                }
                let retry_after = error
                    .split("Retry-After: ")
                    .nth(1)
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(delay)
                    .clamp(1, 3600);
                update(&c, "reconnecting", Some(error));
                let until = Instant::now() + Duration::from_secs(retry_after);
                while current(&c, epoch) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(250));
                }
                delay = (delay * 2).min(60);
            }
        })
        .map(|_| ())
        .map_err(|_| "无法启动 Slack 接收线程".into())
}

pub(crate) fn socket_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "Slack Socket 地址无效")?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("Slack Socket 地址无效".into());
    }
    if url.scheme() == "wss"
        && url
            .host_str()
            .is_some_and(|host| host.ends_with(".slack.com"))
        && url.port_or_known_default() == Some(443)
    {
        return Ok(url);
    }
    if let Ok(origin) = std::env::var("OMNISTREAM_PLUGIN_TEST_ORIGIN") {
        if let Ok(origin) = Url::parse(&origin) {
            if origin.scheme() == "http"
                && matches!(origin.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
                && url.scheme() == "ws"
                && url.host_str() == origin.host_str()
                && url.port_or_known_default() == origin.port_or_known_default()
                && url.path() == "/slack/socket"
                && url.query().is_none()
            {
                return Ok(url);
            }
        }
    }
    Err("Socket Mode 只能连接 Slack 官方 WSS 地址".into())
}

fn connect(url: &Url) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let host = url.host_str().ok_or("Slack Socket 地址无效")?;
    let addresses = (
        host,
        url.port_or_known_default().ok_or("Slack Socket 地址无效")?,
    )
        .to_socket_addrs()
        .map_err(|_| "无法解析 Slack Socket 地址")?;
    let stream = addresses
        .filter_map(|address| TcpStream::connect_timeout(&address, Duration::from_secs(5)).ok())
        .next()
        .ok_or("无法连接 Slack Socket")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| "无法配置 Slack Socket")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| "无法配置 Slack Socket")?;
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    // One handshake, with no redirects. The URL is private and never appears in errors.
    let (socket, _) = tungstenite::client_tls_with_config(url.as_str(), stream, Some(config), None)
        .map_err(|_| "Slack Socket 握手失败")?;
    match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(Duration::from_secs(1))),
        MaybeTlsStream::Rustls(stream) => {
            stream.sock.set_read_timeout(Some(Duration::from_secs(1)))
        }
        _ => return Err("Slack Socket 加密方式不受支持".into()),
    }
    .map_err(|_| "无法配置 Slack Socket")?;
    Ok(socket)
}

fn receive(c: &Connection, epoch: u64, url: Url) -> Result<(), String> {
    let mut socket = connect(&url)?;
    let inbox = Inbox::open(&crate::state_path())?;
    let mut seen = Instant::now();
    let mut ping = Instant::now();
    while current(c, epoch) {
        match socket.read() {
            Ok(Message::Text(text)) => {
                seen = Instant::now();
                let envelope: Value =
                    serde_json::from_str(&text).map_err(|_| "Slack 返回无效事件")?;
                if envelope["type"] == "hello" {
                    update(c, "listening", None);
                }
                if envelope["type"] == "disconnect" {
                    return Err("Slack 要求重新连接".into());
                }
                if let Some(id) = envelope["envelope_id"].as_str() {
                    if id.len() > 200 {
                        return Err("Slack 事件标识无效".into());
                    }
                    let imported = if envelope["type"] == "events_api" {
                        crate::plugin_marketplace::with_slack_current(c, || {
                            inbox.ingest(c, &envelope["payload"])
                        })?
                    } else {
                        Some(false)
                    };
                    let Some(imported) = imported else {
                        break;
                    };
                    socket
                        .send(Message::Text(json!({"envelope_id":id}).to_string().into()))
                        .map_err(|_| "无法确认 Slack 事件")?;
                    if imported {
                        if let Ok(mut status) = STATUS.lock() {
                            if status.generation == c.generation {
                                status.last_event_at = Some(now());
                            }
                        }
                    }
                }
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                seen = Instant::now();
                socket.flush().map_err(|_| "Slack 心跳失败")?;
            }
            Ok(Message::Close(_)) => return Err("Slack 连接已关闭".into()),
            Ok(_) => (),
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                ()
            }
            Err(_) => return Err("Slack Socket 连接中断".into()),
        }
        if seen.elapsed() > Duration::from_secs(75) {
            return Err("Slack 心跳超时".into());
        }
        if ping.elapsed() > Duration::from_secs(20) {
            socket
                .send(Message::Ping(vec![].into()))
                .map_err(|_| "Slack 心跳失败")?;
            ping = Instant::now();
        }
    }
    let _ = socket.close(None);
    Ok(())
}

pub fn api(method: &str, path: &str, body: &[u8]) -> Result<Value, (String, String)> {
    if !matches!(
        (method, path),
        ("GET", "/plugins/slack/inbox")
            | (
                "POST",
                "/plugins/slack/inbox/read"
                    | "/plugins/slack/inbox/send"
                    | "/plugins/slack/listener/reconnect"
            )
    ) {
        return Err(("405 Method Not Allowed".into(), "不支持的请求方法".into()));
    }
    if path == "/plugins/slack/inbox/send" && method == "POST" {
        return crate::plugin_marketplace::send_slack(body);
    }
    let result = (|| -> Result<Value, String> {
        let connection = crate::plugin_marketplace::slack_connection()?;
        if path == "/plugins/slack/listener/reconnect" && method == "POST" {
            RECONNECT.fetch_add(1, Ordering::Relaxed);
            return Ok(json!({"reconnecting":true}));
        }
        if path == "/plugins/slack/inbox" && method == "GET" {
            let mut status = STATUS
                .lock()
                .map_err(|_| "无法读取 Slack 监听状态")?
                .clone();
            if let Some(c) = &connection {
                if !active(c) {
                    status.state = "stopped";
                    status.error = None;
                } else if status.generation != c.generation {
                    status.state = "connecting";
                    status.error = None;
                    status.last_event_at = None;
                    status.connected_at = None;
                }
            } else {
                status.state = "stopped";
                status.error = None;
                status.last_event_at = None;
                status.connected_at = None;
            }
            let conversations = if let Some(c) = connection.as_ref().filter(|c| {
                c.auth_mode == AuthMode::SlackUser && c.user_id.is_some() && c.team_id.is_some()
            }) {
                Inbox::open(&crate::state_path())?.conversations(c)?
            } else {
                json!([])
            };
            return Ok(
                json!({"connection":Summary::from_connection(connection.as_ref()),"listener":status,"conversations":conversations}),
            );
        }
        if path == "/plugins/slack/inbox/read" && method == "POST" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Read {
                channel: String,
                before: Option<String>,
                limit: Option<u64>,
            }
            let input: Read = serde_json::from_slice(body).map_err(|_| "收件箱请求参数无效")?;
            let c = connection
                .as_ref()
                .filter(|c| c.auth_mode == AuthMode::SlackUser)
                .ok_or("请先连接 Slack 个人账号")?;
            return Inbox::open(&crate::state_path())?.messages(
                c,
                &input.channel,
                input.before.as_deref(),
                input.limit.unwrap_or(50),
            );
        }
        Err("不支持的 Slack 收件箱请求".into())
    })();
    result.map_err(|error| ("400 Bad Request".into(), error))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_addresses_allow_official_tls_hosts_and_never_arbitrary_hosts_or_credentials() {
        assert!(socket_url("wss://wss-primary.slack.com/link/private-ticket").is_ok());
        for url in [
            "ws://wss-primary.slack.com/link/ticket",
            "wss://slack.com.evil.test/link/ticket",
            "wss://evil.test/link/ticket",
            "wss://user:password@wss-primary.slack.com/link/ticket",
            "wss://wss-primary.slack.com:444/link/ticket",
            "wss://wss-primary.slack.com/link/ticket#fragment",
        ] {
            let error = socket_url(url).unwrap_err();
            assert!(!error.contains("ticket"));
        }
    }
}
