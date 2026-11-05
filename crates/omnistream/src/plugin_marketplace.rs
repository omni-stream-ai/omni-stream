use crate::plugin::{Manifest, Registry};
use crate::{
    config::ApprovalMode,
    plugin_connections::{AuthMode, Connection, ListenScope, Store, Summary},
    plugin_connectors::{self, Action, Connector, ServiceError},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex, time::Duration};

// Only filesystem snapshots/commits hold this lock; upstream I/O never does.
// Revisions include disconnects with no saved credentials, so an old connection
// attempt cannot restore a connection after the user has disconnected it.
struct RegistryState {
    revisions: BTreeMap<(PathBuf, String), u64>,
}
impl RegistryState {
    fn revision(&self, registry: &Registry, id: &str) -> u64 {
        self.revisions
            .get(&(registry.root().to_owned(), id.into()))
            .copied()
            .unwrap_or(0)
    }
    fn advance(&mut self, registry: &Registry, id: &str) {
        *self
            .revisions
            .entry((registry.root().to_owned(), id.into()))
            .or_default() += 1;
    }
    fn check(&self, registry: &Registry, id: &str, expected: u64) -> Result<(), ApiError> {
        if self.revision(registry, id) != expected {
            return Err((
                "409 Conflict".into(),
                "插件设置已变化，旧验证结果未保存，请重新检查连接设置".into(),
            ));
        }
        require_installed(registry, id).map_err(|error| ("409 Conflict".into(), error))
    }
}
static REGISTRY_LOCK: Mutex<RegistryState> = Mutex::new(RegistryState {
    revisions: BTreeMap::new(),
});

#[derive(Serialize)]
struct Listing {
    manifest: Manifest,
    installed: bool,
    installed_version: Option<String>,
    connection: Summary,
    actions: Vec<Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    browser: Option<crate::browser_mcp::Summary>,
}

fn catalog() -> Vec<Manifest> {
    [
        ("slack", "Slack"),
        ("github", "GitHub"),
        ("notion", "Notion"),
        ("figma", "Figma"),
        (crate::browser_mcp::ID, "Chrome DevTools MCP"),
    ]
    .into_iter()
    .map(|(id, name)| Manifest {
        id: id.into(),
        name: name.into(),
        version: if id == crate::browser_mcp::ID {
            crate::browser_mcp::VERSION
        } else {
            "0.1.0"
        }
        .into(),
        runtime: if id == crate::browser_mcp::ID {
            "mcp"
        } else {
            "builtin"
        }
        .into(),
        capabilities: vec![if id == crate::browser_mcp::ID {
            "browser.automation".into()
        } else {
            format!("integration.{id}")
        }],
        permissions: if id == crate::browser_mcp::ID {
            vec!["browser.control".into()]
        } else {
            vec![format!("{id}.read"), format!("{id}.write")]
        },
        actions: plugin_connectors::actions(id)
            .iter()
            .map(|action| action.id.into())
            .collect(),
    })
    .collect()
}

type ApiError = (String, String);
fn internal(message: String) -> ApiError {
    ("500 Internal Server Error".into(), message)
}
fn not_found() -> ApiError {
    ("404 Not Found".into(), "插件或接口不存在".into())
}
fn method_not_allowed() -> ApiError {
    ("405 Method Not Allowed".into(), "不支持的请求方法".into())
}

pub fn api(method: &str, path: &str, body: &[u8]) -> Result<Value, ApiError> {
    handle_body(&Registry::global(), method, path, body)
}

pub(crate) fn setup_name(id: &str) -> Result<String, ApiError> {
    let manifest = catalog()
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(not_found)?;
    if manifest.runtime != "builtin" {
        return Err(not_found());
    }
    let _guard = REGISTRY_LOCK
        .lock()
        .map_err(|error| internal(error.to_string()))?;
    require_installed(&Registry::global(), id).map_err(|error| ("409 Conflict".into(), error))?;
    Ok(manifest.name)
}

#[cfg(test)]
fn handle(registry: &Registry, method: &str, path: &str) -> Result<serde_json::Value, ApiError> {
    handle_body(registry, method, path, &[])
}

fn connections(registry: &Registry) -> Store {
    Store::new(registry.root().join("connections"))
}
pub(crate) fn slack_connection() -> Result<Option<Connection>, String> {
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "无法读取 Slack 连接")?;
    let registry = Registry::global();
    if require_installed(&registry, "slack").is_err() {
        return Ok(None);
    }
    connections(&registry).load("slack")
}
pub(crate) fn with_slack_current<T>(
    expected: &Connection,
    apply: impl FnOnce() -> Result<T, String>,
) -> Result<Option<T>, String> {
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "无法读取 Slack 连接")?;
    let registry = Registry::global();
    if require_installed(&registry, "slack").is_err() {
        return Ok(None);
    }
    let current = connections(&registry).load("slack")?;
    if !current.is_some_and(|c| {
        c.generation == expected.generation
            && c.enabled
            && c.listen
            && c.auth_mode == AuthMode::SlackUser
    }) {
        return Ok(None);
    }
    apply().map(Some)
}
pub(crate) fn send_slack(body: &[u8]) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Send {
        team_id: String,
        user_id: String,
        channel: String,
        text: String,
        thread_ts: Option<String>,
    }
    let request: Send = parse(body)?;
    let mut input = json!({"channel":request.channel,"text":request.text});
    if let Some(ts) = request.thread_ts {
        input["thread_ts"] = json!(ts);
    }
    let registry = Registry::global();
    let connection = {
        let _guard = REGISTRY_LOCK
            .lock()
            .map_err(|_| internal("无法读取 Slack 连接".into()))?;
        let (c, _) = ready(&registry, "slack", "send_message", &input)
            .map_err(|e| ("409 Conflict".into(), e))?;
        if c.auth_mode != AuthMode::SlackUser
            || c.team_id.as_deref() != Some(&request.team_id)
            || c.user_id.as_deref() != Some(&request.user_id)
        {
            return Err((
                "409 Conflict".into(),
                "Slack 身份已变化，请刷新收件箱后发送".into(),
            ));
        }
        c
    };
    invoke(&registry, "slack", "send_message", &input, &connection)
        .map_err(|e| ("502 Bad Gateway".into(), e))
}
fn summary(registry: &Registry, id: &str) -> Summary {
    if id == crate::browser_mcp::ID {
        return Summary::from_connection(None);
    }
    match connections(registry).load(id) {
        Ok(connection) => Summary::from_connection(connection.as_ref()),
        Err(message) => Summary {
            status: "error",
            auth_mode: AuthMode::Token,
            app_token_configured: false,
            listen: false,
            listen_scope: ListenScope::Related,
            user_id: None,
            team_id: None,
            configured: false,
            enabled: false,
            allow_writes: false,
            account: None,
            verified_at: None,
            last_error: Some(message),
        },
    }
}
fn listing(registry: &Registry, manifest: Manifest) -> Result<Value, ApiError> {
    let installed_version = registry
        .discover()
        .map_err(internal)?
        .iter()
        .find(|item| item.id == manifest.id)
        .map(|item| item.version.clone());
    let actions = plugin_connectors::actions(&manifest.id);
    let connection = summary(registry, &manifest.id);
    let browser =
        (manifest.id == crate::browser_mcp::ID).then(|| crate::browser_mcp::summary(registry));
    serde_json::to_value(Listing {
        manifest,
        installed: installed_version.is_some(),
        installed_version,
        connection,
        actions,
        browser,
    })
    .map_err(|e| internal(e.to_string()))
}
fn require_installed(registry: &Registry, id: &str) -> Result<(), String> {
    if !registry.discover()?.iter().any(|item| item.id == id) {
        return Err("请先在插件市场下载此插件".into());
    }
    Ok(())
}

pub(crate) fn browser_enabled() -> Result<bool, String> {
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "无法读取浏览器设置")?;
    let registry = Registry::global();
    if !registry
        .discover()?
        .iter()
        .any(|item| item.id == crate::browser_mcp::ID)
    {
        return Ok(false);
    }
    Ok(crate::browser_mcp::load(&registry)?.enabled)
}

pub(crate) fn agent_snapshot() -> Result<crate::agent_plugins::Snapshot, String> {
    use crate::agent_plugins::{Grant, Snapshot};
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "无法读取插件设置")?;
    let registry = Registry::global();
    let mut snapshot = Snapshot::default();
    for manifest in registry.discover()? {
        if manifest.id == crate::browser_mcp::ID {
            snapshot.browser = crate::browser_mcp::load(&registry)?.enabled;
        } else if !plugin_connectors::actions(&manifest.id).is_empty() {
            if let Some(connection) = connections(&registry)
                .load(&manifest.id)?
                .filter(|c| c.enabled)
            {
                snapshot.services.insert(
                    manifest.id,
                    Grant {
                        generation: connection.generation,
                        allow_writes: connection.allow_writes,
                    },
                );
            }
        }
    }
    Ok(snapshot)
}
fn ready(
    registry: &Registry,
    id: &str,
    action: &str,
    input: &Value,
) -> Result<(Connection, Action), String> {
    require_installed(registry, id)?;
    let descriptor = plugin_connectors::validate(id, action, input)?;
    let connection = connections(registry)
        .load(id)?
        .ok_or("请先在插件市场配置访问令牌")?;
    if !connection.enabled {
        return Err("插件已停用，请先启用".into());
    }
    if descriptor.write && !connection.allow_writes {
        return Err("此插件未开启写入权限".into());
    }
    Ok((connection, descriptor))
}
fn service_error(
    registry: &Registry,
    id: &str,
    connection: &Connection,
    error: ServiceError,
) -> String {
    if error.auth_failed {
        if let Ok(Some(mut current)) = connections(registry).load(id) {
            if current.token == connection.token && current.generation == connection.generation {
                current.last_error = Some(error.message.clone());
                let _ = connections(registry).save(id, &current);
            }
        }
    }
    error.message
}
fn invoke(
    registry: &Registry,
    id: &str,
    name: &str,
    input: &Value,
    connection: &Connection,
) -> Result<Value, String> {
    invoke_outcome(registry, id, name, input, connection).map_err(|error| error.message)
}
fn invoke_outcome(
    registry: &Registry,
    id: &str,
    name: &str,
    input: &Value,
    connection: &Connection,
) -> Result<Value, ServiceError> {
    let mut result = Connector::new(id, &connection.token)?
        .invoke(name, input)
        .map_err(|error| {
            let _guard = REGISTRY_LOCK.lock().ok();
            let definitely_not_applied = error.definitely_not_applied;
            let retry_after = error.retry_after;
            let auth_failed = error.auth_failed;
            ServiceError {
                message: service_error(registry, id, connection, error),
                definitely_not_applied,
                retry_after,
                auth_failed,
            }
        })?;
    if id == "slack"
        && name == "send_message"
        && connection.auth_mode == AuthMode::SlackUser
        && connection.user_id.is_some()
        && connection.team_id.is_some()
    {
        let synced = (|| -> Result<(), String> {
            let _guard = REGISTRY_LOCK.lock().map_err(|_| "无法读取 Slack 连接")?;
            if connections(registry)
                .load(id)?
                .is_some_and(|c| c.generation == connection.generation)
            {
                crate::slack_inbox::Inbox::open(&crate::state_path())?
                    .sent(connection, input, &result)?;
            }
            Ok(())
        })();
        if synced.is_err() {
            result["sync_warning"] =
                json!("消息已发送，但本地收件箱保存失败，请先检查 Slack，避免重复发送");
        }
    }
    Ok(result)
}
fn parse<T: for<'a> Deserialize<'a>>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|_| ("400 Bad Request".into(), "插件请求参数格式无效".into()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configure {
    token: Option<String>,
    auth_mode: Option<AuthMode>,
    app_token: Option<String>,
    listen: Option<bool>,
    listen_scope: Option<ListenScope>,
    enabled: bool,
    allow_writes: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {
    action: String,
    input: Value,
}

fn handle_body(
    registry: &Registry,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<Value, ApiError> {
    if matches!(
        path,
        "/plugins/slack/inbox"
            | "/plugins/slack/inbox/read"
            | "/plugins/slack/inbox/send"
            | "/plugins/slack/listener/reconnect"
    ) {
        return crate::slack::api(method, path, body);
    }
    let mut guard = REGISTRY_LOCK
        .lock()
        .map_err(|error| internal(error.to_string()))?;
    if path == "/plugins/marketplace" {
        if method != "GET" {
            return Err(method_not_allowed());
        }
        let installed = registry.discover().map_err(internal)?;
        let listings: Vec<_> = catalog()
            .into_iter()
            .map(|manifest| {
                let version = installed
                    .iter()
                    .find(|item| item.id == manifest.id)
                    .map(|item| item.version.clone());
                Listing {
                    browser: (manifest.id == crate::browser_mcp::ID)
                        .then(|| crate::browser_mcp::summary(registry)),
                    connection: summary(registry, &manifest.id),
                    actions: plugin_connectors::actions(&manifest.id),
                    manifest,
                    installed: version.is_some(),
                    installed_version: version,
                }
            })
            .collect();
        return serde_json::to_value(listings).map_err(|e| internal(e.to_string()));
    }

    let segments: Vec<_> = path
        .strip_prefix("/plugins/")
        .ok_or_else(not_found)?
        .split('/')
        .collect();
    let (id, operation) = match segments.as_slice() {
        [id, operation @ ("install" | "connection" | "test" | "invoke")] => (*id, *operation),
        [id] => (*id, "uninstall"),
        _ => return Err(not_found()),
    };
    let manifest = catalog()
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(not_found)?;
    match (method, operation) {
        ("POST", "install") => {
            registry.install(&manifest).map_err(internal)?;
            guard.advance(registry, id);
        }
        ("DELETE", "uninstall") => {
            if id == crate::browser_mcp::ID {
                crate::browser_mcp::remove(registry).map_err(internal)?;
            } else {
                connections(registry).remove(id).map_err(internal)?;
            }
            guard.advance(registry, id);
            registry.uninstall(id).map_err(internal)?;
        }
        ("POST", "connection") if id == crate::browser_mcp::ID => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Enable {
                enabled: bool,
            }
            require_installed(registry, id).map_err(|e| ("409 Conflict".into(), e))?;
            let config: Enable = parse(body)?;
            let previous = crate::browser_mcp::load(registry).unwrap_or_default();
            let state = crate::browser_mcp::State {
                enabled: config.enabled,
                verified_at: if previous.enabled == config.enabled {
                    previous.verified_at
                } else {
                    None
                },
                last_error: None,
            };
            crate::browser_mcp::save(registry, &state).map_err(internal)?;
            guard.advance(registry, id);
        }
        ("DELETE", "connection") if id == crate::browser_mcp::ID => {
            crate::browser_mcp::remove(registry).map_err(internal)?;
            guard.advance(registry, id);
        }
        ("POST", "test") if id == crate::browser_mcp::ID => {
            require_installed(registry, id).map_err(|e| ("409 Conflict".into(), e))?;
            if !crate::browser_mcp::load(registry)
                .map_err(internal)?
                .enabled
            {
                return Err(("409 Conflict".into(), "请先启用 Chrome 浏览器工具".into()));
            }
            let revision = guard.revision(registry, id);
            drop(guard);
            let result = crate::browser_mcp::probe(true);
            guard = REGISTRY_LOCK.lock().map_err(|e| internal(e.to_string()))?;
            guard.check(registry, id, revision)?;
            let state = crate::browser_mcp::State {
                enabled: true,
                verified_at: result.is_ok().then(now),
                last_error: result.clone().err(),
            };
            crate::browser_mcp::save(registry, &state).map_err(internal)?;
            result.map_err(|error| ("502 Bad Gateway".into(), error))?;
        }
        ("DELETE", "connection") => {
            connections(registry).remove(id).map_err(internal)?;
            guard.advance(registry, id);
        }
        ("POST", "connection") => {
            require_installed(registry, id).map_err(|e| ("409 Conflict".into(), e))?;
            let config: Configure = parse(body)?;
            let previous = connections(registry).load(id).ok().flatten();
            let auth_mode = config
                .auth_mode
                .or_else(|| previous.as_ref().map(|c| c.auth_mode))
                .unwrap_or_default();
            if id != "slack" && auth_mode == AuthMode::SlackUser {
                return Err(("400 Bad Request".into(), "此连接方式仅适用于 Slack".into()));
            }
            let same_mode = previous.as_ref().is_some_and(|c| c.auth_mode == auth_mode);
            let listen = config
                .listen
                .unwrap_or_else(|| same_mode && previous.as_ref().is_some_and(|c| c.listen));
            let listen_scope = config
                .listen_scope
                .or_else(|| {
                    previous
                        .as_ref()
                        .filter(|_| same_mode)
                        .map(|c| c.listen_scope)
                })
                .unwrap_or_default();
            let app_token = config
                .app_token
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    previous
                        .as_ref()
                        .filter(|_| same_mode)
                        .and_then(|c| c.app_token.clone())
                });
            if (listen || app_token.is_some())
                && (id != "slack" || auth_mode != AuthMode::SlackUser)
            {
                return Err((
                    "400 Bad Request".into(),
                    "实时监听仅适用于 Slack 个人账号".into(),
                ));
            }
            if app_token.as_ref().is_some_and(|value| {
                !value.starts_with("xapp-")
                    || value.len() > 8192
                    || value.chars().any(|c| c.is_whitespace() || c.is_control())
            }) || (listen && app_token.is_none())
            {
                return Err((
                    "400 Bad Request".into(),
                    "监听需要有效的 App-level Token（xapp-）".into(),
                ));
            }
            let token = config
                .token
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    previous
                        .as_ref()
                        .filter(|_| same_mode)
                        .map(|c| c.token.clone())
                })
                .ok_or_else(|| {
                    (
                        "400 Bad Request".into(),
                        "请填写访问令牌；切换连接方式需要新令牌".into(),
                    )
                })?;
            if token.len() > 8192 || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(("400 Bad Request".into(), "访问令牌格式无效".into()));
            }
            if auth_mode == AuthMode::SlackUser && !token.starts_with("xoxp-") {
                return Err((
                    "400 Bad Request".into(),
                    "个人账号方式需要 User OAuth Token（xoxp-）".into(),
                ));
            }
            let unchanged = same_mode && previous.as_ref().is_some_and(|c| c.token == token);
            let mut connection = Connection {
                token,
                auth_mode,
                app_token,
                listen,
                listen_scope,
                user_id: None,
                team_id: None,
                generation: format!(
                    "{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ),
                enabled: config.enabled,
                allow_writes: config.allow_writes,
                account: None,
                verified_at: None,
                last_error: None,
            };
            if unchanged {
                let old = previous.unwrap();
                connection.account = old.account;
                connection.verified_at = old.verified_at;
                connection.last_error = old.last_error;
                connection.user_id = old.user_id;
                connection.team_id = old.team_id;
            }
            if connection.enabled {
                let connector = Connector::new(id, &connection.token).map_err(internal)?;
                let revision = guard.revision(registry, id);
                drop(guard);
                let verified =
                    connector
                        .verify_identity(connection.auth_mode)
                        .and_then(|identity| {
                            if connection.listen {
                                if identity.user_id.is_none() || identity.team_id.is_none() {
                                    return Err("Slack 未返回有效的用户和工作区身份".into());
                                }
                                Connector::new("slack", connection.app_token.as_deref().unwrap())?
                                    .open_socket()?;
                            }
                            Ok(identity)
                        });
                guard = REGISTRY_LOCK
                    .lock()
                    .map_err(|error| internal(error.to_string()))?;
                guard.check(registry, id, revision)?;
                let identity =
                    verified.map_err(|error| ("502 Bad Gateway".into(), error.message))?;
                connection.account = Some(identity.account);
                connection.user_id = identity.user_id;
                connection.team_id = identity.team_id;
                connection.verified_at = Some(now());
                connection.last_error = None;
            }
            connections(registry)
                .save(id, &connection)
                .map_err(internal)?;
            guard.advance(registry, id);
        }
        ("POST", "test") => {
            require_installed(registry, id).map_err(|e| ("409 Conflict".into(), e))?;
            let mut connection = connections(registry)
                .load(id)
                .map_err(internal)?
                .ok_or_else(|| ("409 Conflict".into(), "请先配置访问令牌".into()))?;
            let revision = guard.revision(registry, id);
            let connector = Connector::new(id, &connection.token).map_err(internal)?;
            drop(guard);
            let result = connector
                .verify_identity(connection.auth_mode)
                .and_then(|identity| {
                    if connection.listen {
                        Connector::new(
                            "slack",
                            connection
                                .app_token
                                .as_deref()
                                .ok_or("请填写 App-level Token")?,
                        )?
                        .open_socket()?;
                    }
                    Ok(identity)
                });
            guard = REGISTRY_LOCK
                .lock()
                .map_err(|error| internal(error.to_string()))?;
            guard.check(registry, id, revision)?;
            match result {
                Ok(identity) => {
                    connection.account = Some(identity.account);
                    connection.user_id = identity.user_id;
                    connection.team_id = identity.team_id;
                    connection.verified_at = Some(now());
                    connection.last_error = None;
                    connections(registry)
                        .save(id, &connection)
                        .map_err(internal)?;
                }
                Err(error) => {
                    connection.last_error = Some(error.message.clone());
                    connections(registry)
                        .save(id, &connection)
                        .map_err(internal)?;
                    return Err(("502 Bad Gateway".into(), error.message));
                }
            }
        }
        ("POST", "invoke") => {
            let invocation: Invocation = parse(body)?;
            let (connection, _) = ready(registry, id, &invocation.action, &invocation.input)
                .map_err(|e| ("409 Conflict".into(), e))?;
            drop(guard);
            return invoke(
                registry,
                id,
                &invocation.action,
                &invocation.input,
                &connection,
            )
            .map_err(|e| ("502 Bad Gateway".into(), e));
        }
        _ => return Err(method_not_allowed()),
    }
    listing(registry, manifest)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn context() -> Value {
    match api("GET","/plugins/marketplace",&[]) {
        Ok(Value::Array(items))=>Value::Array(items.into_iter().map(|item|{
            let ready=item["installed"]==true && item["connection"]["enabled"]==true;
            let actions=if ready {item["actions"].as_array().unwrap().iter().filter(|action|action["write"]!=true || item["connection"]["allow_writes"]==true).cloned().collect::<Vec<_>>()} else {vec![]};
            json!({"id":item["manifest"]["id"],"name":item["manifest"]["name"],"runtime":item["manifest"]["runtime"],"installed":item["installed"],"connection":item["connection"],"browser":item["browser"],"actions":actions})
        }).collect()),
        Ok(value)=>value,
        Err((_,error))=>json!({"error":error}),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolInvocation {
    plugin: String,
    action: String,
    input: Value,
}
/// A manager request may perform several distinct writes, but never replay one.
/// Keep failed attempts too: a transport error may follow a committed write.
#[derive(Default)]
pub struct TurnWrites {
    attempts: Vec<(Value, Value)>,
}
impl TurnWrites {
    pub fn execute(
        &mut self,
        name: &str,
        args: &Value,
        execute: impl FnOnce() -> Value,
    ) -> Result<Value, String> {
        let write = name == "plugin_invoke"
            && plugin_connectors::actions(args["plugin"].as_str().unwrap_or_default())
                .iter()
                .any(|action| {
                    action.id == args["action"].as_str().unwrap_or_default() && action.write
                });
        if write {
            if let Some((_, result)) = self.attempts.iter().find(|(previous, _)| previous == args) {
                return Err(format!(
                    "已阻止同一请求内的重复插件写入，执行已停止。首次调用结果：{result}。请以首次结果判断是否成功；错误或超时不代表服务端未生效，未再次发送请求。"
                ));
            }
        }
        let result = execute();
        if write {
            self.attempts.push((args.clone(), result.clone()));
        }
        Ok(result)
    }
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, "list_plugins" | "plugin_invoke")
}
/// Public account summary and revocable connection reference, never a token.
pub(crate) fn delivery_grant(
    plugin: &str,
    action: &str,
    input: &Value,
) -> Result<(String, Value), String> {
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "无法读取插件设置")?;
    let (connection, _) = ready(&Registry::global(), plugin, action, input)?;
    if plugin_connectors::delivery_destination_fields(plugin, action).is_none() {
        return Err("不允许后台投递此动作".into());
    }
    Ok((
        connection.generation.clone(),
        json!({"account":connection.account,"auth_mode":connection.auth_mode,"user_id":connection.user_id,"team_id":connection.team_id}),
    ))
}
/// Only the scheduler calls this after checking the persisted task grant and claiming a receipt.
pub(crate) fn deliver_authorized(
    plugin: &str,
    action: &str,
    input: &Value,
    generation: &str,
) -> Result<Value, (String, String, Option<u64>)> {
    let registry = Registry::global();
    let connection = (|| {
        let _guard = REGISTRY_LOCK.lock().map_err(|_| "无法读取插件设置")?;
        let (connection, _) = ready(&registry, plugin, action, input)?;
        if plugin_connectors::delivery_destination_fields(plugin, action).is_none()
            || connection.generation != generation
        {
            return Err("投递授权已失效，需重新确认账号和权限".into());
        }
        Ok::<_, String>(connection)
    })()
    .map_err(|e| ("failed".into(), e, None))?;
    invoke_outcome(&registry, plugin, action, input, &connection).map_err(|error| {
        let status = if error.definitely_not_applied {
            "failed"
        } else {
            "unknown"
        };
        let retry = if error.definitely_not_applied {
            error.retry_after
        } else {
            None
        };
        (status.into(), error.message, retry)
    })
}

pub fn tool(
    name: &str,
    args: &Value,
    conversation: &str,
    mode: ApprovalMode,
) -> Result<Value, String> {
    tool_controlled(name, args, conversation, mode, None, None, None, &|| false)
}

#[allow(clippy::too_many_arguments)]
pub fn tool_controlled(
    name: &str,
    args: &Value,
    conversation: &str,
    mode: ApprovalMode,
    scope: Option<&str>,
    expected_generation: Option<&str>,
    task_id: Option<u64>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value, String> {
    if cancelled() {
        return Err("插件调用已取消或执行结束".into());
    }
    if name == "list_plugins" {
        return Ok(context());
    }
    if name != "plugin_invoke" {
        return Err("未知插件工具".into());
    }
    let call: ToolInvocation = serde_json::from_value(args.clone())
        .map_err(|_| "插件工具需要 plugin、action 和 input 参数")?;
    let registry = Registry::global();
    let (connection, descriptor) = {
        let _guard = REGISTRY_LOCK.lock().map_err(|e| e.to_string())?;
        ready(&registry, &call.plugin, &call.action, &call.input)?
    };
    if expected_generation.is_some_and(|expected| expected != connection.generation) {
        return Err("插件连接或权限已变化，请重新启动 Agent 执行".into());
    }
    let store = crate::storage::Store::open(&crate::state_path())?;
    let approval = if descriptor.write && mode != ApprovalMode::All {
        let id=store.create_approval(task_id,"plugin.invoke",&json!({"conversation":conversation,"scope":scope,"plugin":call.plugin,"action":call.action,"input":call.input,"account":connection.account,"requested_at":now()}))?;
        println!(
            "插件写入请求 #{id}：{} {}；approval approve {id} / approval reject {id}",
            call.plugin, call.action
        );
        await_approval_controlled(&store, id, cancelled)?;
        Some(id)
    } else {
        None
    };
    let result = (|| {
        if cancelled() {
            return Err("插件调用已取消或执行结束".into());
        }
        // Approval applies only to the account and permission snapshot the user reviewed.
        let (current, _) = {
            let _guard = REGISTRY_LOCK.lock().map_err(|e| e.to_string())?;
            ready(&registry, &call.plugin, &call.action, &call.input)?
        };
        if current.generation != connection.generation
            || current.auth_mode != connection.auth_mode
            || current.token != connection.token
            || current.allow_writes != connection.allow_writes
        {
            return Err("插件连接或权限已变化，请重新提交操作".into());
        }
        invoke(&registry, &call.plugin, &call.action, &call.input, &current)
    })();
    if let Some(id) = approval {
        store.finish_approval(
            id,
            &result.as_ref().map(Value::to_string).map_err(Clone::clone),
        )?;
    }
    result
}

#[cfg(test)]
fn await_approval(store: &crate::storage::Store, id: i64) -> Result<(), String> {
    await_approval_controlled(store, id, &|| false)
}
fn await_approval_controlled(
    store: &crate::storage::Store,
    id: i64,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    loop {
        if cancelled() {
            store.expire_agent_approval(id)?;
            return Err("插件写入审批已取消或执行结束".into());
        }
        match store.approval(id)?.ok_or("插件审批不存在")?.status.as_str() {
            "approved" => {
                return if store.claim_approved_plugin(id)? {
                    Ok(())
                } else {
                    Err("插件审批已被处理".into())
                }
            }
            "rejected" => return Err("用户已拒绝插件写入操作".into()),
            "pending" => std::thread::sleep(Duration::from_millis(50)),
            _ => return Err("插件审批已失效".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_mode_changes_require_new_tokens_and_keep_the_previous_connection_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::new(dir.path());
        for id in ["slack", "github"] {
            handle(&registry, "POST", &format!("/plugins/{id}/install")).unwrap();
        }
        let configure = |id: &str, body: Value| {
            handle_body(
                &registry,
                "POST",
                &format!("/plugins/{id}/connection"),
                &serde_json::to_vec(&body).unwrap(),
            )
        };
        configure(
            "slack",
            json!({"token":"old-bot","enabled":false,"allow_writes":false}),
        )
        .unwrap();
        for body in [
            json!({"auth_mode":"slack_user","enabled":false,"allow_writes":true}),
            json!({"auth_mode":"slack_user","token":"xoxb-bot","enabled":false,"allow_writes":true}),
            json!({"auth_mode":"invalid","enabled":false,"allow_writes":false}),
        ] {
            assert_eq!(configure("slack", body).unwrap_err().0, "400 Bad Request");
            assert_eq!(
                connections(&registry).load("slack").unwrap().unwrap().token,
                "old-bot"
            );
        }
        let saved = configure("slack", json!({"auth_mode":"slack_user","token":"xoxp-fixture-user","enabled":false,"allow_writes":false})).unwrap();
        assert_eq!(saved["connection"]["auth_mode"], "slack_user");
        assert!(!saved.to_string().contains("xoxp-fixture-user"));
        let generation = connections(&registry)
            .load("slack")
            .unwrap()
            .unwrap()
            .generation;
        configure("slack", json!({"enabled":false,"allow_writes":false})).unwrap();
        let current = connections(&registry).load("slack").unwrap().unwrap();
        assert!(current.auth_mode == AuthMode::SlackUser);
        assert_eq!(current.token, "xoxp-fixture-user");
        assert_ne!(current.generation, generation);
        assert_eq!(configure("github", json!({"auth_mode":"slack_user","token":"xoxp-fixture-user","enabled":false,"allow_writes":false})).unwrap_err().0, "400 Bad Request");
        assert!(ready(
            &registry,
            "slack",
            "channel_history",
            &json!({"channel":"D123"})
        )
        .err()
        .unwrap()
        .contains("停用"));
    }

    #[test]
    fn plugin_approvals_are_once_only_and_expire_on_cancel_or_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open(&dir.path().join("state.db")).unwrap();
        let id = store
            .create_approval(None, "plugin.invoke", &json!({"conversation":"a"}))
            .unwrap();
        store.resolve_approval(id, "approved").unwrap();
        await_approval(&store, id).unwrap();
        assert!(await_approval(&store, id).is_err());
        store.finish_approval(id, &Ok("done".into())).unwrap();
        assert_eq!(store.approval(id).unwrap().unwrap().status, "succeeded");
        let rejected = store
            .create_approval(None, "plugin.invoke", &json!({}))
            .unwrap();
        store.resolve_approval(rejected, "rejected").unwrap();
        assert!(await_approval(&store, rejected)
            .unwrap_err()
            .contains("拒绝"));
        let expired = store
            .create_approval(None, "plugin.invoke", &json!({}))
            .unwrap();
        assert!(await_approval_controlled(&store, expired, &|| true)
            .unwrap_err()
            .contains("取消"));
        assert_eq!(store.approval(expired).unwrap().unwrap().status, "expired");
        let pending = store
            .create_approval(None, "plugin.invoke", &json!({}))
            .unwrap();
        let external = store
            .create_approval(None, "agent.request", &json!({}))
            .unwrap();
        store.expire_plugin_approvals().unwrap();
        assert_eq!(store.approval(pending).unwrap().unwrap().status, "expired");
        assert_eq!(store.approval(external).unwrap().unwrap().status, "pending");
    }

    #[test]
    fn catalog_is_opt_in_and_survives_reopening_the_registry() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::new(dir.path());
        let entries = handle(&registry, "GET", "/plugins/marketplace").unwrap();
        assert_eq!(entries.as_array().unwrap().len(), 5);
        assert!(entries
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["installed"] == false));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        for id in ["slack", "github", "notion", "figma"] {
            for _ in 0..2 {
                let listing = handle(&registry, "POST", &format!("/plugins/{id}/install")).unwrap();
                assert_eq!(listing["installed"], true);
                assert_eq!(listing["installed_version"], "0.1.0");
            }
        }
        let reopened = Registry::new(dir.path());
        assert_eq!(reopened.discover().unwrap().len(), 4);
        for _ in 0..2 {
            let removed = handle(&reopened, "DELETE", "/plugins/github").unwrap();
            assert_eq!(removed["installed"], false);
            assert!(removed["installed_version"].is_null());
        }
        assert_eq!(reopened.discover().unwrap().len(), 3);
        assert!(dir.path().join("slack.json").is_file());
    }

    #[test]
    fn errors_do_not_install_unknown_plugins_or_hide_registry_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::new(dir.path());
        for path in [
            "/plugins/unknown/install",
            "/plugins/../install",
            "/plugins/%2e%2e/install",
            "/plugins/slack/extra/install",
        ] {
            assert_eq!(
                handle(&registry, "POST", path).unwrap_err().0,
                "404 Not Found"
            );
        }
        assert_eq!(
            handle(&registry, "GET", "/plugins/slack/install")
                .unwrap_err()
                .0,
            "405 Method Not Allowed"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        std::fs::write(dir.path().join("broken.json"), "{").unwrap();
        assert_eq!(
            handle(&registry, "GET", "/plugins/marketplace")
                .unwrap_err()
                .0,
            "500 Internal Server Error"
        );
        std::fs::remove_file(dir.path().join("broken.json")).unwrap();
        std::fs::create_dir(dir.path().join("slack.json")).unwrap();
        assert_eq!(
            handle(&registry, "POST", "/plugins/slack/install")
                .unwrap_err()
                .0,
            "500 Internal Server Error"
        );
        assert_eq!(
            handle(&registry, "DELETE", "/plugins/slack").unwrap_err().0,
            "500 Internal Server Error"
        );
        assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| entry
            .unwrap()
            .path()
            .extension()
            .unwrap()
            != "tmp"));
    }
}

#[cfg(test)]
mod turn_write_tests {
    use super::*;

    fn send(channel: &str) -> Value {
        json!({"plugin":"slack","action":"send_message","input":{"channel":channel,"text":"你好"}})
    }

    #[test]
    fn repeated_successful_decisions_execute_only_once() {
        let mut writes = TurnWrites::default();
        let mut calls = 0;
        let result = json!({"data":{"ok":true,"ts":"1793789654.816139"}});
        for index in 0..11 {
            let output = writes.execute("plugin_invoke", &send("C123"), || {
                calls += 1;
                result.clone()
            });
            if index == 0 {
                assert_eq!(output.unwrap(), result);
            } else {
                let error = output.unwrap_err();
                assert!(error.contains("1793789654.816139"));
                assert!(error.contains("已阻止"));
            }
        }
        assert_eq!(calls, 1);
    }

    #[test]
    fn failed_or_uncertain_write_is_not_retried_even_with_reordered_keys() {
        let mut writes = TurnWrites::default();
        let args = send("C123");
        writes
            .execute("plugin_invoke", &args, || json!({"error":"timeout"}))
            .unwrap();
        let reordered = serde_json::from_str(r#"{"input":{"text":"你好","channel":"C123"},"action":"send_message","plugin":"slack"}"#).unwrap();
        let error = writes
            .execute("plugin_invoke", &reordered, || {
                panic!("must not call service or request approval again")
            })
            .unwrap_err();
        assert!(error.contains("timeout"));
    }

    #[test]
    fn distinct_writes_reads_and_new_requests_remain_available() {
        let mut writes = TurnWrites::default();
        let open = json!({"plugin":"slack","action":"open_conversation","input":{"users":"U123"}});
        let read = json!({"plugin":"slack","action":"list_channels","input":{}});
        for args in [open, send("C123"), read.clone(), read, send("C456")] {
            writes
                .execute("plugin_invoke", &args, || json!({"data":{"ok":true}}))
                .unwrap();
        }
        TurnWrites::default()
            .execute(
                "plugin_invoke",
                &send("C123"),
                || json!({"data":{"ok":true}}),
            )
            .unwrap();
    }
}
