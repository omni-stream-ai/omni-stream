//! Built-in service clients. URLs and operations are allowlisted here, never supplied by callers.
use crate::plugin_connections::AuthMode;
use reqwest::{blocking::Client, Method, Url};
use serde::Serialize;
use serde_json::{json, Value};
use std::{io::Read, time::Duration};

#[derive(Clone, Serialize)]
pub struct Action {
    pub id: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'static str>,
    pub write: bool,
    pub delivery_destination_fields: Option<Vec<&'static str>>,
    pub input_schema: Value,
}

#[cfg(test)]
#[path = "plugin_connectors_tests.rs"]
mod tests;

fn text(max: u64) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn identifier() -> Value {
    json!({"type":"string","minLength":1,"maxLength":200,"pattern":"^[A-Za-z0-9_-][A-Za-z0-9_.-]*$"})
}
fn cursor() -> Value {
    json!({"type":"string","minLength":1})
}
fn timestamp() -> Value {
    json!({"type":"string","minLength":1,"maxLength":50,"pattern":"^[0-9]+(?:\\.[0-9]+)?$"})
}
fn number(default: u64, max: u64) -> Value {
    json!({"type":"integer","minimum":1,"maximum":max,"default":default})
}
fn choice(values: &[&str], default: &str) -> Value {
    json!({"type":"string","enum":values,"default":default})
}
fn action(id: &'static str, write: bool, fields: Vec<(&str, Value, bool)>) -> Action {
    let required: Vec<_> = fields
        .iter()
        .filter(|(_, _, required)| *required)
        .map(|(key, _, _)| *key)
        .collect();
    let properties: serde_json::Map<_, _> = fields
        .into_iter()
        .map(|(key, schema, _)| (key.into(), schema))
        .collect();
    Action {
        id,
        description: match id {
            "list_users" => Some("List workspace members to resolve a name to a user ID (users:read required). Match name, real_name or profile display_name/real_name. Continue with next_cursor when needed; a single page is not the whole directory. Preserve ambiguous matches and deleted/is_bot flags; do not guess a recipient. After confirming the user, use open_conversation then send_message."),
            "user_info" => Some("Read a member profile by U/W user ID (users:read required). Use to verify identity; D/C conversation IDs are not user IDs."),
            _ => None,
        },
        write,
        delivery_destination_fields: None,
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}
pub fn actions(id: &str) -> Vec<Action> {
    let mut actions = match id {
        "slack" => vec![
            action(
                "list_channels",
                false,
                vec![
                    (
                        "types",
                        choice(
                            &["public_channel", "private_channel", "im", "mpim"],
                            "public_channel",
                        ),
                        false,
                    ),
                    ("limit", number(20, 100), false),
                    ("cursor", cursor(), false),
                ],
            ),
            action(
                "list_users",
                false,
                vec![
                    ("limit", number(100, 200), false),
                    ("cursor", cursor(), false),
                ],
            ),
            action(
                "user_info",
                false,
                vec![(
                    "user",
                    json!({"type":"string","minLength":1,"maxLength":200,"pattern":"^[UW][A-Z0-9]+$"}),
                    true,
                )],
            ),
            action(
                "channel_history",
                false,
                vec![
                    ("channel", identifier(), true),
                    ("oldest", timestamp(), false),
                    ("latest", timestamp(), false),
                    ("limit", number(15, 15), false),
                    ("cursor", cursor(), false),
                ],
            ),
            action(
                "thread_replies",
                false,
                vec![
                    ("channel", identifier(), true),
                    ("ts", timestamp(), true),
                    ("limit", number(15, 15), false),
                    ("cursor", cursor(), false),
                ],
            ),
            action(
                "open_conversation",
                true,
                vec![(
                    "users",
                    json!({"type":"string","minLength":1,"maxLength":200,"pattern":"^[UW][A-Z0-9]+(?:,[UW][A-Z0-9]+){0,7}$"}),
                    true,
                )],
            ),
            action(
                "send_message",
                true,
                vec![
                    ("channel", identifier(), true),
                    ("text", text(4000), true),
                    ("thread_ts", text(50), false),
                ],
            ),
        ],
        "github" => vec![
            action(
                "list_repositories",
                false,
                vec![
                    ("per_page", number(20, 100), false),
                    ("page", number(1, 10000), false),
                ],
            ),
            action(
                "get_repository",
                false,
                vec![("owner", identifier(), true), ("repo", identifier(), true)],
            ),
            action(
                "list_issues",
                false,
                vec![
                    ("owner", identifier(), true),
                    ("repo", identifier(), true),
                    ("state", choice(&["open", "closed", "all"], "open"), false),
                    ("per_page", number(20, 100), false),
                    ("page", number(1, 10000), false),
                ],
            ),
            action(
                "list_pull_requests",
                false,
                vec![
                    ("owner", identifier(), true),
                    ("repo", identifier(), true),
                    ("state", choice(&["open", "closed", "all"], "open"), false),
                    ("per_page", number(20, 100), false),
                    ("page", number(1, 10000), false),
                ],
            ),
            action(
                "create_issue",
                true,
                vec![
                    ("owner", identifier(), true),
                    ("repo", identifier(), true),
                    ("title", text(256), true),
                    ("body", text(65536), false),
                ],
            ),
        ],
        "notion" => vec![
            action(
                "search",
                false,
                vec![
                    ("query", text(200), false),
                    ("page_size", number(20, 100), false),
                    ("start_cursor", cursor(), false),
                ],
            ),
            action("get_page", false, vec![("page_id", identifier(), true)]),
            action(
                "list_blocks",
                false,
                vec![
                    ("block_id", identifier(), true),
                    ("page_size", number(20, 100), false),
                    ("start_cursor", cursor(), false),
                ],
            ),
            action(
                "create_page",
                true,
                vec![
                    ("parent_page_id", identifier(), true),
                    ("title", text(2000), true),
                    ("content", text(2000), false),
                ],
            ),
        ],
        "figma" => vec![
            action(
                "get_file",
                false,
                vec![
                    ("file_key", identifier(), true),
                    ("depth", number(2, 10), false),
                ],
            ),
            action(
                "get_comments",
                false,
                vec![("file_key", identifier(), true)],
            ),
            action(
                "post_comment",
                true,
                vec![
                    ("file_key", identifier(), true),
                    ("message", text(10000), true),
                    ("comment_id", identifier(), false),
                ],
            ),
        ],
        _ => vec![],
    };
    for action in &mut actions {
        action.delivery_destination_fields =
            delivery_destination_fields(id, action.id).map(|fields| fields.to_vec());
    }
    actions
}

/// Connector-owned delivery capability; other writes require separate interactive authorization.
pub fn delivery_destination_fields(plugin: &str, action: &str) -> Option<&'static [&'static str]> {
    match (plugin, action) {
        ("slack", "send_message") => Some(&["channel", "thread_ts"]),
        ("github", "create_issue") => Some(&["owner", "repo"]),
        ("notion", "create_page") => Some(&["parent_page_id"]),
        ("figma", "post_comment") => Some(&["file_key", "comment_id"]),
        _ => None,
    }
}
pub fn validate(id: &str, name: &str, input: &Value) -> Result<Action, String> {
    let action = actions(id)
        .into_iter()
        .find(|action| action.id == name)
        .ok_or("插件不支持此操作")?;
    let validator =
        jsonschema::validator_for(&action.input_schema).map_err(|_| "插件参数定义无效")?;
    if let Err(error) = validator.validate(input) {
        return Err(format!(
            "插件参数无效（{}）：{}",
            error.instance_path, error
        ));
    }
    Ok(action)
}

#[derive(Debug)]
pub struct ServiceError {
    pub message: String,
    pub auth_failed: bool,
    pub definitely_not_applied: bool,
    pub retry_after: Option<u64>,
}
impl From<String> for ServiceError {
    fn from(message: String) -> Self {
        Self {
            message,
            auth_failed: false,
            definitely_not_applied: false,
            retry_after: None,
        }
    }
}
impl From<&str> for ServiceError {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

pub fn redact(value: &mut Value, token: &str) {
    match value {
        Value::String(value) if !token.is_empty() => *value = value.replace(token, "[redacted]"),
        Value::Array(values) => values.iter_mut().for_each(|value| redact(value, token)),
        Value::Object(values) => {
            values.retain(|key, _| {
                ![
                    "access_token",
                    "refresh_token",
                    "token",
                    "authorization",
                    "secret",
                ]
                .contains(&key.to_ascii_lowercase().as_str())
            });
            values.values_mut().for_each(|value| redact(value, token));
        }
        _ => (),
    }
}

pub struct Connector {
    base: Url,
    client: Client,
    id: String,
    token: String,
}
pub struct Identity {
    pub account: String,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
}
impl Connector {
    pub fn new(id: &str, token: &str) -> Result<Self, String> {
        let base = match id {
            "slack" => "https://slack.com/api",
            "github" => "https://api.github.com",
            "notion" => "https://api.notion.com/v1",
            "figma" => "https://api.figma.com/v1",
            _ => return Err("未知插件".into()),
        };
        let base = if let Ok(origin) = std::env::var("OMNISTREAM_PLUGIN_TEST_ORIGIN") {
            // Test transport is process-only and constrained to a loopback HTTP server.
            let url = Url::parse(&origin).map_err(|_| "插件测试地址无效")?;
            if url.scheme() != "http"
                || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("插件测试地址只能是本地回环 HTTP 服务根地址".into());
            }
            format!("{}/{id}", origin.trim_end_matches('/'))
        } else {
            base.into()
        };
        Self::with_base(id, token, &base)
    }
    pub(crate) fn with_base(id: &str, token: &str, base: &str) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("OmniStream/0.1")
            .build()
            .map_err(|_| "无法创建插件连接")?;
        Ok(Self {
            base: Url::parse(base).map_err(|_| "插件地址无效")?,
            client,
            id: id.into(),
            token: token.into(),
        })
    }
    fn request(
        &self,
        method: Method,
        path: &[&str],
        query: &[(String, String)],
        body: Option<Value>,
    ) -> Result<Value, ServiceError> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| "插件地址无效")?
            .pop_if_empty()
            .extend(path);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let mut request = self.client.request(method, url).header(
            "Accept",
            if self.id == "github" {
                "application/vnd.github+json"
            } else {
                "application/json"
            },
        );
        request = if self.id == "figma" {
            request.header("X-Figma-Token", &self.token)
        } else {
            request.bearer_auth(&self.token)
        };
        if self.id == "github" {
            request = request.header("X-GitHub-Api-Version", "2026-03-10");
        }
        if self.id == "notion" {
            request = request.header("Notion-Version", "2026-03-11");
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().map_err(|error| {
            ServiceError::from(if error.is_timeout() {
                "服务请求超时；写入操作请先检查服务端结果，避免重复提交"
            } else {
                "无法访问插件服务，请检查网络连接"
            })
        })?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|header| header.to_str().ok())
            .map(str::to_owned);
        let next_page = response
            .headers()
            .get("link")
            .and_then(|header| header.to_str().ok())
            .and_then(|links| {
                links
                    .split(',')
                    .find(|link| link.contains("rel=\"next\""))
                    .and_then(|link| link.split('<').nth(1))
                    .and_then(|link| link.split('>').next())
                    .and_then(|url| Url::parse(url).ok())
                    .and_then(|url| {
                        url.query_pairs()
                            .find(|(key, _)| key == "page")
                            .and_then(|(_, value)| value.parse::<u64>().ok())
                    })
            });
        const MAX: u64 = 4 * 1024 * 1024;
        let mut bytes = Vec::new();
        response
            .take(MAX + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "无法读取服务响应")?;
        if bytes.len() as u64 > MAX {
            return Err("服务响应超过 4 MiB，请缩小查询范围".into());
        }
        let mut data: Value = match serde_json::from_slice(&bytes) {
            Ok(data) => data,
            Err(_) if !status.is_success() => {
                return Err(ServiceError {
                    message: format!(
                        "{} HTTP {}{}",
                        self.id,
                        status.as_u16(),
                        retry_after
                            .as_ref()
                            .map(|value| format!(
                                "；Retry-After: {}",
                                value.chars().take(40).collect::<String>()
                            ))
                            .unwrap_or_default()
                    )
                    .replace(&self.token, "[redacted]"),
                    auth_failed: status.as_u16() == 401,
                    definitely_not_applied: matches!(status.as_u16(), 401 | 403 | 429),
                    retry_after: if status.as_u16() == 429 {
                        retry_after
                            .as_deref()
                            .and_then(|s| s.parse::<u64>().ok())
                            .filter(|s| *s <= 60)
                            .map(|s| s.max(1))
                    } else {
                        None
                    },
                })
            }
            Err(_) => {
                return Err(ServiceError::from(format!(
                    "服务返回无效 JSON（HTTP {}）",
                    status.as_u16()
                )))
            }
        };
        redact(&mut data, &self.token);
        if status.is_success() && self.id == "slack" && !data["ok"].is_boolean() {
            return Err("Slack 响应缺少有效的 ok 布尔字段；服务端结果不明".into());
        }
        if !status.is_success() || (self.id == "slack" && data["ok"] == false) {
            let code = data["error"]
                .as_str()
                .or_else(|| data["code"].as_str())
                .unwrap_or_default();
            let detail = data["message"]
                .as_str()
                .or_else(|| data["err"].as_str())
                .unwrap_or(code);
            let detail: String = detail.chars().take(500).collect();
            let retry_delay = if status.as_u16() == 429 {
                retry_after
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|s| *s <= 60)
                    .map(|s| s.max(1))
            } else {
                None
            };
            let retry = retry_after
                .map(|value| {
                    format!(
                        "；Retry-After: {}",
                        value.chars().take(40).collect::<String>()
                    )
                })
                .unwrap_or_default();
            return Err(ServiceError {
                definitely_not_applied: matches!(status.as_u16(), 401 | 403 | 429)
                    || (self.id == "slack"
                        && [
                            "channel_not_found",
                            "not_in_channel",
                            "missing_scope",
                            "invalid_auth",
                            "not_authed",
                            "token_expired",
                            "token_revoked",
                            "account_inactive",
                            "is_archived",
                            "msg_too_long",
                            "no_text",
                        ]
                        .contains(&code)),
                retry_after: retry_delay,
                message: format!("{} HTTP {}：{}{retry}", self.id, status.as_u16(), detail)
                    .replace(&self.token, "[redacted]"),
                auth_failed: status.as_u16() == 401
                    || [
                        "invalid_auth",
                        "not_authed",
                        "token_expired",
                        "token_revoked",
                        "account_inactive",
                    ]
                    .contains(&code),
            });
        }
        let next_cursor = data
            .pointer("/response_metadata/next_cursor")
            .or_else(|| data.get("next_cursor"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Ok(json!({"data":data,"next_cursor":next_cursor,"next_page":next_page}))
    }
    #[cfg(test)]
    pub fn verify(&self) -> Result<String, ServiceError> {
        self.verify_mode(AuthMode::Token)
    }
    #[cfg(test)]
    pub fn verify_mode(&self, auth_mode: AuthMode) -> Result<String, ServiceError> {
        self.verify_identity(auth_mode)
            .map(|identity| identity.account)
    }
    pub fn verify_identity(&self, auth_mode: AuthMode) -> Result<Identity, ServiceError> {
        if auth_mode == AuthMode::SlackUser
            && (self.id != "slack" || !self.token.starts_with("xoxp-"))
        {
            return Err("个人账号方式需要 Slack User OAuth Token（xoxp-），不能使用 Bot Token 或 CLI 配置令牌".into());
        }
        let path: &[&str] = match self.id.as_str() {
            "slack" => &["auth.test"],
            "github" => &["user"],
            "notion" => &["users", "me"],
            _ => &["me"],
        };
        let result = self.request(
            if self.id == "slack" {
                Method::POST
            } else {
                Method::GET
            },
            path,
            &[],
            None,
        )?;
        if auth_mode == AuthMode::SlackUser
            && (result["data"]["bot_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty())
                || result["data"]["user_id"]
                    .as_str()
                    .is_none_or(|id| id.is_empty()))
        {
            return Err("Slack 返回的不是有效用户身份，请使用本人授权的 User OAuth Token".into());
        }
        Ok(Identity {
            account: self.identity(&result["data"])?,
            user_id: result["data"]["user_id"].as_str().map(str::to_owned),
            team_id: result["data"]["team_id"].as_str().map(str::to_owned),
        })
    }
    pub fn open_socket(&self) -> Result<Url, ServiceError> {
        if self.id != "slack" || !self.token.starts_with("xapp-") {
            return Err("实时监听需要 App-level Token（xapp-）及 connections:write 权限".into());
        }
        let result = self.request(Method::POST, &["apps.connections.open"], &[], None)?;
        let url = result["data"]["url"]
            .as_str()
            .ok_or("Slack 未返回 Socket Mode 地址")?;
        crate::slack::socket_url(url).map_err(Into::into)
    }
    fn identity(&self, data: &Value) -> Result<String, ServiceError> {
        let account = match self.id.as_str() {
            "slack" => data["user"].as_str().or_else(|| data["user_id"].as_str()),
            "github" => data["login"].as_str(),
            "notion" => data["name"]
                .as_str()
                .or_else(|| data.pointer("/bot/workspace_name").and_then(Value::as_str)),
            _ => data["handle"].as_str(),
        }
        .or_else(|| data["id"].as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or("服务未返回有效账号信息")?;
        Ok(account.chars().take(200).collect())
    }
    pub fn invoke(&self, name: &str, input: &Value) -> Result<Value, ServiceError> {
        validate(&self.id, name, input)?;
        let s = |key: &str| input[key].as_str().unwrap_or_default();
        let n = |key: &str, default| input[key].as_u64().unwrap_or(default).to_string();
        let page = || {
            vec![
                ("per_page".into(), n("per_page", 20)),
                ("page".into(), n("page", 1)),
            ]
        };
        let mut query = vec![];
        let mut body = None;
        let mut method = Method::GET;
        let parts: Vec<&str> = match (self.id.as_str(), name) {
            ("slack", "list_channels") => {
                query = vec![
                    ("limit".into(), n("limit", 20)),
                    ("exclude_archived".into(), "true".into()),
                    (
                        "types".into(),
                        input["types"].as_str().unwrap_or("public_channel").into(),
                    ),
                ];
                vec!["conversations.list"]
            }
            ("slack", "list_users") => {
                query = vec![("limit".into(), n("limit", 100))];
                vec!["users.list"]
            }
            ("slack", "user_info") => {
                query = vec![("user".into(), s("user").into())];
                vec!["users.info"]
            }
            ("slack", "channel_history") => {
                query = vec![
                    ("channel".into(), s("channel").into()),
                    ("limit".into(), n("limit", 15)),
                ];
                for key in ["oldest", "latest"] {
                    if let Some(value) = input[key].as_str() {
                        query.push((key.into(), value.into()));
                    }
                }
                vec!["conversations.history"]
            }
            ("slack", "thread_replies") => {
                query = vec![
                    ("channel".into(), s("channel").into()),
                    ("ts".into(), s("ts").into()),
                    ("limit".into(), n("limit", 15)),
                ];
                vec!["conversations.replies"]
            }
            ("slack", "open_conversation") => {
                method = Method::POST;
                body = Some(json!({"users":s("users")}));
                vec!["conversations.open"]
            }
            ("slack", "send_message") => {
                method = Method::POST;
                body = Some(json!({"channel":s("channel"),"text":s("text")}));
                if !s("thread_ts").is_empty() {
                    body.as_mut().unwrap()["thread_ts"] = input["thread_ts"].clone();
                }
                vec!["chat.postMessage"]
            }
            ("github", "list_repositories") => {
                query = page();
                vec!["user", "repos"]
            }
            ("github", "get_repository") => vec!["repos", s("owner"), s("repo")],
            ("github", "list_issues" | "list_pull_requests") => {
                query = page();
                query.push((
                    "state".into(),
                    input["state"].as_str().unwrap_or("open").into(),
                ));
                vec![
                    "repos",
                    s("owner"),
                    s("repo"),
                    if name == "list_issues" {
                        "issues"
                    } else {
                        "pulls"
                    },
                ]
            }
            ("github", "create_issue") => {
                method = Method::POST;
                body = Some(json!({"title":s("title"),"body":s("body")}));
                vec!["repos", s("owner"), s("repo"), "issues"]
            }
            ("notion", "search") => {
                method = Method::POST;
                let mut value = json!({"page_size":input["page_size"].as_u64().unwrap_or(20)});
                for key in ["query", "start_cursor"] {
                    if input.get(key).is_some() {
                        value[key] = input[key].clone();
                    }
                }
                body = Some(value);
                vec!["search"]
            }
            ("notion", "get_page") => vec!["pages", s("page_id")],
            ("notion", "list_blocks") => {
                query = vec![("page_size".into(), n("page_size", 20))];
                if !s("start_cursor").is_empty() {
                    query.push(("start_cursor".into(), s("start_cursor").into()));
                }
                vec!["blocks", s("block_id"), "children"]
            }
            ("notion", "create_page") => {
                method = Method::POST;
                let mut value = json!({"parent":{"type":"page_id","page_id":s("parent_page_id")},"properties":{"title":{"type":"title","title":[{"type":"text","text":{"content":s("title")}}]}}});
                if !s("content").is_empty() {
                    value["children"] = json!([{"object":"block","type":"paragraph","paragraph":{"rich_text":[{"type":"text","text":{"content":s("content")}}]}}]);
                }
                body = Some(value);
                vec!["pages"]
            }
            ("figma", "get_file") => {
                query = vec![("depth".into(), n("depth", 2))];
                vec!["files", s("file_key")]
            }
            ("figma", "get_comments") => vec!["files", s("file_key"), "comments"],
            ("figma", "post_comment") => {
                method = Method::POST;
                let mut value = json!({"message":s("message")});
                if !s("comment_id").is_empty() {
                    value["comment_id"] = input["comment_id"].clone();
                } else {
                    value["client_meta"] = json!({"x":0,"y":0});
                }
                body = Some(value);
                vec!["files", s("file_key"), "comments"]
            }
            _ => return Err("插件不支持此操作".into()),
        };
        if self.id == "slack" && !s("cursor").is_empty() {
            query.push(("cursor".into(), s("cursor").into()));
        }
        let result = self.request(method, &parts, &query, body)?;
        self.validate_delivery_response(name, &result["data"])?;
        Ok(result)
    }
    fn validate_delivery_response(&self, action: &str, data: &Value) -> Result<(), ServiceError> {
        let nonempty = |key: &str| data[key].as_str().is_some_and(|s| !s.trim().is_empty());
        let positive = |key: &str| data[key].as_u64().is_some_and(|n| n > 0);
        let valid = match (self.id.as_str(), action) {
            ("slack", "send_message") => {
                data["ok"] == true
                    && nonempty("channel")
                    && nonempty("ts")
                    && jsonschema::validator_for(&timestamp())
                        .map_err(|_| "时间戳定义无效")?
                        .is_valid(&data["ts"])
            }
            ("github", "create_issue") => positive("id") && positive("number"),
            ("notion", "create_page") => data["object"] == "page" && nonempty("id"),
            ("figma", "post_comment") => nonempty("id"),
            _ if delivery_destination_fields(&self.id, action).is_some() => false,
            _ => return Ok(()),
        };
        if valid {
            Ok(())
        } else {
            Err(format!(
                "{} {} 响应缺少有效的成功标识；服务端结果不明，禁止自动重发",
                self.id, action
            )
            .into())
        }
    }
}
