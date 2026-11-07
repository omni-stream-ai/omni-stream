//! Credential-free setup assistance. Credentials are submitted only to /connection.
use crate::{
    config::Config,
    delegation::Pending,
    plugin_connections::{AuthMode, ListenScope},
    storage::Store,
    App,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

type ApiError = (String, String);
static SETUP_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en-US")]
    English,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub auth_mode: AuthMode,
    pub listen: bool,
    pub listen_scope: ListenScope,
    pub allow_writes: bool,
    pub enabled: bool,
    pub locale: Language,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    request_id: String,
    options: Options,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    plugin: String,
    options: Options,
    agent: String,
}

pub fn matches(path: &str) -> bool {
    path.strip_prefix("/plugins/").is_some_and(|suffix| {
        matches!(
            suffix.split('/').collect::<Vec<_>>().as_slice(),
            [_, "setup"]
        )
    })
}

fn error(status: &str, message: impl Into<String>) -> ApiError {
    (status.into(), message.into())
}

fn validate(id: &str, options: &Options) -> Result<(), String> {
    if id != "slack" && (options.auth_mode != AuthMode::Token || options.listen) {
        return Err("个人账号和持续接收选项仅适用于 Slack".into());
    }
    if options.listen && options.auth_mode != AuthMode::SlackUser {
        return Err("持续接收需要 Slack 个人账号方式".into());
    }
    Ok(())
}

fn request_id_valid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, byte)| {
            if [8, 13, 18, 23].contains(&i) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn pending(
    app: &Arc<Mutex<App>>,
    conversation: &str,
    context: &Context,
    request: &str,
    history: &[Value],
) -> Result<(Pending, String), String> {
    let mut config: Config = app.lock().map_err(|_| "Core 状态锁定失败")?.config.clone();
    config.approval_mode = Some(
        Store::open(&crate::state_path())?
            .conversation_settings(conversation)?
            .approval_mode,
    );
    config.plugin_setup = true;
    config.agents.retain(|_, agent| {
        !agent.command.trim().is_empty()
            && crate::adapter::Protocol::parse(agent.protocol.as_deref()).is_ok()
    });
    if !context.agent.is_empty() {
        config.agents.retain(|name, _| name == &context.agent);
    }
    let browser = crate::agent_plugins::attach(&mut config)?;
    if browser {
        config.agents.retain(|_, agent| agent.plugin_tools.browser);
        if config.agents.is_empty() {
            return Err("没有启用插件工具的 Agent；请检查 agents 的 plugin_transport 配置，或使用手动配置说明".into());
        }
    }
    if config.agents.is_empty() {
        return Err("没有可用的配置 Agent，请在 Core 的 config.toml 中配置 [agents.<名称>] 的 command 和 protocol；也可展开手动配置说明。".into());
    }
    let project = crate::state_path()
        .parent()
        .ok_or("状态目录无效")?
        .join("workspace")
        .join("plugin-setup");
    std::fs::create_dir_all(&project).map_err(|e| e.to_string())?;
    let project = project.to_string_lossy().into_owned();
    let mut prompt = instructions(&context.plugin, &context.options);
    if browser {
        prompt.push_str(crate::browser_mcp::setup_hint());
    }
    let background = json!({"setup":context,"history":history}).to_string();
    let pending = Pending::with_config(
        config,
        project.clone(),
        conversation,
        &format!("{prompt}\n用户请求：{request}"),
        "协助配置插件",
        &background,
    )?;
    Ok((pending, project))
}

pub fn api(
    app: &Arc<Mutex<App>>,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<Value, ApiError> {
    let id = path
        .strip_prefix("/plugins/")
        .and_then(|p| p.strip_suffix("/setup"))
        .ok_or_else(|| error("404 Not Found", "插件或接口不存在"))?;
    let name = crate::plugin_marketplace::setup_name(id)?;
    if method != "POST" {
        return Err(error("405 Method Not Allowed", "不支持的请求方法"));
    }
    // Never echo deserialization errors: malformed values may themselves contain credentials.
    let input: Start = serde_json::from_slice(body).map_err(|_| {
        error(
            "400 Bad Request",
            "配置协助只接受 request_id 和非敏感 options，不接受令牌或其他字段",
        )
    })?;
    if !request_id_valid(&input.request_id) {
        return Err(error("400 Bad Request", "request_id 必须为 UUID"));
    }
    validate(id, &input.options).map_err(|e| error("400 Bad Request", e))?;
    let _guard = SETUP_LOCK
        .lock()
        .map_err(|_| error("500 Internal Server Error", "配置协助状态锁定失败"))?;
    let conversation = format!("plugin-setup:{id}:{}", input.request_id);
    let store =
        Store::open(&crate::state_path()).map_err(|e| error("500 Internal Server Error", e))?;
    let history = store
        .conversation(&conversation)
        .map_err(|e| error("500 Internal Server Error", e))?;
    if let Some(existing) = history
        .first()
        .and_then(|message| serde_json::from_value::<Context>(message["plugin_setup"].clone()).ok())
    {
        if existing.options != input.options {
            return Err(error(
                "409 Conflict",
                "此次配置协助的选项已变化，请重新发起",
            ));
        }
        let call_id = history
            .iter()
            .find_map(|message| message["call_id"].as_u64())
            .ok_or_else(|| error("409 Conflict", "此前配置协助未能启动，请重新发起"))?;
        return Ok(json!({"conversation":conversation,"call_id":call_id}));
    }
    let mut context = Context {
        plugin: id.into(),
        options: input.options,
        agent: String::new(),
    };
    let request = match context.options.locale {
        Language::Chinese => format!("请协助我配置 {name} 并获取所需令牌；令牌由我直接填写到插件配置。"),
        Language::English => format!("Help me configure {name} and obtain the required tokens. I will enter credentials directly in the plugin configuration."),
    };
    let (pending, project) = pending(app, &conversation, &context, &request, &[])
        .map_err(|e| error("409 Conflict", e))?;
    let index = pending.preferred_index();
    if pending.uses_browser(index) {
        crate::browser_mcp::probe(false).map_err(|e| error("502 Bad Gateway", e))?;
    }
    if crate::plugin_marketplace::browser_enabled()
        .map_err(|e| error("500 Internal Server Error", e))?
        != pending.uses_browser(index)
    {
        return Err(error(
            "409 Conflict",
            "浏览器工具设置已变化，请重新启动配置协助",
        ));
    }
    context.agent = pending
        .selected_name(index)
        .ok_or_else(|| error("409 Conflict", "没有可用 Agent"))?
        .into();
    store
        .rename_conversation(
            &conversation,
            &format!(
                "{name} · {}",
                if context.options.locale == Language::Chinese {
                    "配置协助"
                } else {
                    "Setup assistance"
                }
            ),
        )
        .map_err(|e| error("500 Internal Server Error", e))?;
    store
        .append_message(
            &conversation,
            &json!({"role":"user","content":request,"plugin_setup":context}),
        )
        .map_err(|e| error("500 Internal Server Error", e))?;
    let call_id = crate::start_background_delegation(
        app,
        pending,
        index,
        context.agent,
        project,
        conversation.clone(),
        request,
        None,
    )
    .map_err(|e| error("500 Internal Server Error", e))?;
    Ok(json!({"conversation":conversation,"call_id":call_id}))
}

pub fn continue_conversation(
    app: &Arc<Mutex<App>>,
    conversation: &str,
    value: &Value,
) -> Result<String, String> {
    let _guard = SETUP_LOCK.lock().map_err(|_| "配置协助状态锁定失败")?;
    let store = Store::open(&crate::state_path())?;
    let mut history = store.conversation(conversation)?;
    let context: Context = history
        .first()
        .and_then(|message| serde_json::from_value(message["plugin_setup"].clone()).ok())
        .ok_or("配置协助会话不存在，请从插件配置重新发起")?;
    crate::plugin_marketplace::setup_name(&context.plugin).map_err(|(_, message)| message)?;
    if value["attachments"]
        .as_array()
        .is_some_and(|items| !items.is_empty())
    {
        return Err(
            "配置协助不接收附件；请勿上传含令牌的截图，登录和令牌请在服务及插件配置界面完成。"
                .into(),
        );
    }
    let request = value["messages"]
        .as_array()
        .and_then(|messages| messages.iter().rev().find(|item| item["role"] == "user"))
        .and_then(|message| {
            message["parts"]
                .as_array()
                .and_then(|parts| parts.iter().find_map(|part| part["text"].as_str()))
                .or_else(|| message["content"].as_str())
        })
        .unwrap_or("")
        .trim();
    if request.is_empty() {
        return Err("message 不能为空".into());
    }
    if redact_text(request) != request {
        return Err("请将令牌直接填入插件配置，不要发送到对话。".into());
    }
    for message in &mut history {
        if let Some(id) = message["call_id"].as_u64() {
            if let Some(task) = app
                .lock()
                .map_err(|_| "Core 状态锁定失败")?
                .executions
                .iter()
                .find(|task| task.id == id)
            {
                message["content"] = json!({"status":task.status,"result":task.last_output});
            }
        }
    }
    let (pending, project) = pending(
        app,
        conversation,
        &context,
        request,
        &history[history.len().saturating_sub(20)..],
    )?;
    let index = pending.preferred_index();
    let scope = pending.scope(index);
    let calls = store.conversation_calls(conversation)?;
    let state = app.lock().map_err(|_| "Core 状态锁定失败")?;
    if calls.iter().any(|call| {
        call["scope"] == scope
            && state.executions.iter().any(|task| {
                Some(task.id) == call["task_id"].as_u64()
                    && matches!(task.status.as_str(), "running" | "queued" | "testing")
            })
    }) {
        return Err("该 Agent 会话正在运行，请等待结束或取消后再继续".into());
    }
    drop(state);
    store.append_message(conversation, &json!({"role":"user","content":request}))?;
    let call_id = crate::start_background_delegation(
        app,
        pending,
        index,
        context.agent,
        project,
        conversation.into(),
        request.into(),
        None,
    )?;
    Ok(format!("Agent #{call_id}"))
}

fn instructions(id: &str, options: &Options) -> String {
    let mut text = String::from("这是 OmniStream 插件配置协助。使用你实际具备的浏览器工具帮助用户完成官方配置；没有浏览器工具就明确说明并提供当前一步的链接和指导，不声称已操作。用户负责登录、验证码、授权确认和复制令牌。仅在用户选择的工作区/资源及权限范围内配置，不发送业务消息、不创建演示数据。不要读取本机已有凭据、环境密钥、CLI 登录文件或浏览器 Cookie，不修改 OmniStream 的连接文件。不要请求、读取、复制、输出或截图令牌、密码、验证码和 Socket 票据；遇到凭据页立即让用户接管，将令牌直接填回插件配置的密码框。只报告非敏感配置进度，最后提醒返回配置验证保存，未验证不得声称连接成功。遵守当前会话审批策略，不绕过工作区管理员审批。\n");
    text.push_str(match id {
        "slack" if options.auth_mode == AuthMode::SlackUser => "Slack 个人账号：使用 User OAuth Token（xoxp），让用户选择现有或新建的工作区应用，在 https://api.slack.com/apps 的 OAuth & Permissions → User Token Scopes 配置。按用户实际需要选择 channels:read、channels:history、groups:read、groups:history、im:read、im:history、mpim:read、mpim:history；成员查询另需 users:read；不是 Bot Token Scopes。授权后用户可读取本人有权访问的会话，仍受工作区策略约束。官方说明 https://docs.slack.dev/authentication/installing-with-oauth/ 。",
        "slack" => "Slack Bot：使用 Bot OAuth Token，让用户选择现有或新建的工作区应用，在 https://api.slack.com/apps 的 OAuth & Permissions → Bot Token Scopes 配置 channels:read、channels:history，成员查询另需 users:read，安装到工作区；提醒用户邀请应用加入目标频道。官方说明 https://docs.slack.dev/authentication/tokens/ 。",
        "github" => "GitHub：协助生成有到期时间的细粒度 Personal Access Token，用户选择目标所有者和仓库，仅授予 Metadata、Issues、Pull requests 读取权限。注意组织审批可能尚未完成。官方说明 https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens 。",
        "notion" => "Notion：创建 internal integration，选择用户工作区，授予 Read content，并指导用户将目标页面连接给集成。官方说明 https://developers.notion.com/docs/create-a-notion-integration 。",
        "figma" => "Figma：在用户设置中生成有到期时间的个人访问令牌，需要 current_user:read、file_content:read、file_comments:read。官方说明 https://developers.figma.com/docs/rest-api/authentication/ 。",
        _ => "未知插件，不执行配置。",
    });
    if options.allow_writes {
        text.push_str(match id {
            "slack" if options.auth_mode == AuthMode::SlackUser => "用户选择允许写入：额外请求 User Token 的 chat:write、im:write、mpim:write，用于本人发送和开启会话；本次不发送消息。",
            "slack" => "用户选择允许写入：额外请求 Bot Token 的 chat:write；本次不发送消息。",
            "github" => "用户选择允许写入：Issues 使用读写权限，用于后续创建 Issue。",
            "notion" => "用户选择允许写入：额外授予 Insert content。",
            "figma" => "用户选择允许写入：额外授予 file_comments:write。",
            _ => "",
        });
    } else {
        text.push_str("用户未选择允许写入：不要申请写权限。");
    }
    if options.listen {
        text.push_str("持续接收已选择：在同一 Slack 应用开启 Socket Mode，在 Basic Information → App-level Tokens 创建具有 connections:write 的 App-level Token（xapp），由用户复制到配置页；Event Subscriptions → Subscribe to events on behalf of users 订阅 message.im、message.mpim、message.channels、message.groups，按需要授予对应 User Token 历史权限并重新授权，不使用 Bot events 或公网回调。说明 https://docs.slack.dev/apis/events-api/using-socket-mode/ 。");
        text.push_str(if options.listen_scope == ListenScope::All {
            "监听范围：所有已加入的频道、私信和群聊。"
        } else {
            "监听范围：私信、群聊、@本人及相关线程；OmniStream 在本地过滤相关消息。"
        });
    } else {
        text.push_str("未选择持续接收，不配置 Socket Mode 或 App-level Token。");
    }
    text.push_str(if options.locale == Language::Chinese {
        "用简体中文交流。"
    } else {
        "Respond in English."
    });
    text
}

// Defense in depth for standard credential formats. Setup streaming text is withheld
// until completion so a credential split across deltas cannot escape this filter.
pub fn redact_text(input: &str) -> String {
    const PREFIXES: &[&str] = &[
        "xoxp-",
        "xoxb-",
        "xoxa-",
        "xoxs-",
        "xoxr-",
        "xoxe-",
        "xapp-",
        "github_pat_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "ntn_",
        "secret_",
        "figd_",
    ];
    let mut rest = input;
    let mut result = String::new();
    while let Some((start, prefix)) = PREFIXES
        .iter()
        .filter_map(|prefix| rest.find(prefix).map(|start| (start, *prefix)))
        .min_by_key(|(start, _)| *start)
    {
        result.push_str(&rest[..start]);
        let end = rest[start..]
            .find(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_' | '.' | '='))
            .map(|n| start + n)
            .unwrap_or(rest.len());
        if end > start + prefix.len() {
            result.push_str("[redacted]");
        } else {
            result.push_str(&rest[start..end]);
        }
        rest = &rest[end..];
    }
    result.push_str(rest);
    result
}

pub fn redact_value(value: &mut Value) {
    crate::plugin_connectors::redact(value, "");
    match value {
        Value::String(text) => *text = redact_text(text),
        Value::Array(items) => items.iter_mut().for_each(redact_value),
        Value::Object(items) => items.values_mut().for_each(redact_value),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Options {
        Options {
            auth_mode: AuthMode::Token,
            listen: false,
            listen_scope: ListenScope::Related,
            enabled: true,
            allow_writes: false,
            locale: Language::Chinese,
        }
    }

    #[test]
    fn plugin_setup_rejects_secret_fields_and_invalid_options() {
        let safe = json!({"request_id":"00000000-0000-0000-0000-000000000000","options":options()});
        assert!(serde_json::from_value::<Start>(safe.clone()).is_ok());
        for field in ["token", "app_token", "project", "prompt"] {
            let mut value = safe.clone();
            value[field] = json!("private-value");
            assert!(serde_json::from_value::<Start>(value).is_err());
            let mut value = safe.clone();
            value["options"][field] = json!("private-value");
            assert!(serde_json::from_value::<Start>(value).is_err());
        }
        let mut selected = options();
        selected.listen = true;
        assert!(validate("slack", &selected).is_err());
        selected.auth_mode = AuthMode::SlackUser;
        assert!(validate("slack", &selected).is_ok());
        assert!(validate("github", &selected).is_err());
        assert!(!request_id_valid("../../credentials"));
    }

    #[test]
    fn plugin_setup_instructions_follow_identity_listening_and_write_choices() {
        let mut selected = options();
        for id in ["slack", "github", "notion", "figma"] {
            let text = instructions(id, &selected);
            assert!(text.contains("不要申请写权限"));
            assert!(!text.contains("额外授予"));
            assert!(text.contains("没有浏览器工具"));
        }
        assert!(instructions("slack", &selected).contains("Bot OAuth Token"));
        selected.auth_mode = AuthMode::SlackUser;
        selected.listen = true;
        selected.allow_writes = true;
        let text = instructions("slack", &selected);
        assert!(text.contains("User Token Scopes"));
        assert!(text.contains("on behalf of users"));
        assert!(text.contains("connections:write"));
        assert!(text.contains("chat:write、im:write、mpim:write"));
        assert!(text.contains("@本人及相关线程"));
        selected.listen_scope = ListenScope::All;
        assert!(instructions("slack", &selected).contains("监听范围：所有已加入"));
    }

    #[test]
    fn plugin_setup_redacts_standard_credentials_and_authentication_fields() {
        let mut value = json!({"token":"opaque","authorization":"Bearer opaque","content":"凭据 xoxp-private-user，xapp-private-app github_pat_private figd_private ntn_private secret_private"});
        redact_value(&mut value);
        assert!(value.get("token").is_none());
        assert!(value.get("authorization").is_none());
        assert!(!value.to_string().contains("private"));
        assert!(value["content"]
            .as_str()
            .unwrap()
            .starts_with("凭据 [redacted]，"));
        assert_eq!(redact_text("返回配置后验证保存"), "返回配置后验证保存");
        assert_eq!(
            redact_text("应该使用 xoxp- 还是 xoxb-？"),
            "应该使用 xoxp- 还是 xoxb-？"
        );
    }
}
