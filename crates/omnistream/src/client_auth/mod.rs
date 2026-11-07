mod store;
#[cfg(test)]
mod tests;

use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub require_auth: bool,
}

pub struct Context {
    peer: SocketAddr,
    host: String,
    origin: Option<String>,
    forwarded: bool,
    token: Option<String>,
}

impl Context {
    pub fn parse(peer: SocketAddr, headers: &str) -> Result<Self, String> {
        let mut result = Self {
            peer,
            host: String::new(),
            origin: None,
            forwarded: false,
            token: None,
        };
        let mut sensitive = std::collections::HashSet::new();
        for line in headers.lines().skip(1) {
            let (name, value) = line.split_once(':').ok_or("HTTP 请求头格式无效")?;
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if ["host", "origin", "authorization"].contains(&name.as_str())
                && !sensitive.insert(name.clone())
            {
                return Err("HTTP 请求头重复".into());
            }
            match name.as_str() {
                "host" => result.host = value.to_owned(),
                "origin" => result.origin = Some(value.to_owned()),
                "authorization" => {
                    let (scheme, token) = value.split_once(' ').ok_or("访问凭据格式无效")?;
                    if !scheme.eq_ignore_ascii_case("bearer")
                        || token.is_empty()
                        || token.len() > 200
                        || token.chars().any(char::is_whitespace)
                    {
                        return Err("访问凭据格式无效".into());
                    }
                    result.token = Some(token.to_owned());
                }
                _ if name == "forwarded"
                    || name == "x-real-ip"
                    || name.starts_with("x-forwarded-") =>
                {
                    result.forwarded = true
                }
                _ => (),
            }
        }
        Ok(result)
    }

    fn local(&self) -> bool {
        self.peer.ip().is_loopback()
            && !self.forwarded
            && local_url(&format!("http://{}", self.host), false)
            && self
                .origin
                .as_deref()
                .is_none_or(|origin| local_url(origin, true))
    }
}

fn local_url(value: &str, tauri: bool) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return false;
    }
    if tauri
        && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some("tauri.localhost")))
    {
        return true;
    }
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    })
}

#[derive(Clone)]
pub enum Principal {
    Local,
    Client(String),
}
impl Principal {
    pub fn client_id(&self) -> Option<&str> {
        match self {
            Self::Local => None,
            Self::Client(id) => Some(id),
        }
    }
    fn actor(&self) -> &str {
        self.client_id().unwrap_or("local")
    }
}

pub struct Access {
    path: PathBuf,
    required: bool,
    desktop_hash: Option<String>,
}

pub enum Gate {
    Allow(Principal),
    Response(u16, Value),
}

impl Access {
    pub fn new(path: &Path, required: bool, desktop_token: Option<String>) -> Result<Self, String> {
        store::Store::open(path)?;
        Ok(Self {
            path: path.to_owned(),
            required,
            desktop_hash: desktop_token
                .filter(|s| s.len() >= 32)
                .map(|s| store::hash(&s)),
        })
    }

    fn principal(&self, context: &Context) -> Result<Option<Principal>, String> {
        if !self.required && context.local() {
            return Ok(Some(Principal::Local));
        }
        let Some(token) = context.token.as_deref() else {
            return Ok(None);
        };
        let digest = store::hash(token);
        if self.desktop_hash.as_deref().is_some_and(|expected| {
            expected.len() == digest.len()
                && expected
                    .bytes()
                    .zip(digest.bytes())
                    .fold(0u8, |difference, (a, b)| difference | (a ^ b))
                    == 0
        }) {
            return Ok(Some(Principal::Local));
        }
        Ok(store::Store::open(&self.path)?
            .authorized(token)?
            .map(Principal::Client))
    }

    pub fn still_authorized(&self, principal: &Principal) -> bool {
        match principal {
            Principal::Local => true,
            Principal::Client(id) => store::Store::open(&self.path)
                .and_then(|s| s.active(id))
                .unwrap_or(false),
        }
    }

    pub fn gate(&self, context: &Context, method: &str, path: &str, body: &[u8]) -> Gate {
        match self.dispatch(context, method, path, body) {
            Ok(result) => result,
            Err(message) => Gate::Response(503, json!({"error":message})),
        }
    }

    fn dispatch(
        &self,
        context: &Context,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<Gate, String> {
        let principal = self.principal(context)?;
        if method == "GET" && path == "/health" {
            return Ok(Gate::Response(
                200,
                json!({"ok":true,"access":{"required":self.required || !context.local(),"authorized":principal.is_some(),"client_id":principal.as_ref().and_then(Principal::client_id)}}),
            ));
        }
        if path == "/client-auth/requests" && method == "POST" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Request {
                name: String,
                platform: String,
                token: String,
            }
            if body.len() > 4096 {
                return Ok(Gate::Response(413, json!({"error":"授权申请过大"})));
            }
            let input: Request = match serde_json::from_slice(body) {
                Ok(value) => value,
                Err(_) => return Ok(Gate::Response(400, json!({"error":"授权申请格式无效"}))),
            };
            let result = store::Store::open(&self.path)?.request(
                &input.name,
                &input.platform,
                &context.peer.ip().to_string(),
                context.origin.as_deref().unwrap_or(""),
                &input.token,
            );
            return Ok(match result {
                Ok(record) => Gate::Response(200, json!(record)),
                Err(error) => Gate::Response(
                    if error.contains("频繁") { 429 } else { 400 },
                    json!({"error":error}),
                ),
            });
        }
        if let Some(id) = path
            .strip_prefix("/client-auth/requests/")
            .filter(|id| !id.is_empty() && !id.contains('/'))
        {
            if matches!(method, "GET" | "DELETE") {
                let store = store::Store::open(&self.path)?;
                let own = if let Some(token) = context.token.as_deref() {
                    store.own(id, token)?
                } else {
                    None
                };
                if own.is_none() {
                    return Ok(Gate::Response(404, json!({"error":"授权申请不存在"})));
                }
                let record = if method == "DELETE" {
                    store.cancel(id, context.token.as_deref().unwrap_or_default())?
                } else {
                    own.unwrap()
                };
                return Ok(Gate::Response(200, json!(record)));
            }
        }
        let Some(principal) = principal else {
            return Ok(Gate::Response(
                401,
                json!({"code":"client_authorization_required","error":"客户端尚未授权或授权已撤销"}),
            ));
        };
        if path == "/client-auth/requests" && method == "GET" {
            return Ok(Gate::Response(
                200,
                json!(store::Store::open(&self.path)?.list(true)?),
            ));
        }
        if path == "/client-auth/clients" && method == "GET" {
            return Ok(Gate::Response(
                200,
                json!({"clients":store::Store::open(&self.path)?.list(false)?,"current_id":principal.client_id()}),
            ));
        }
        if let Some(tail) = path.strip_prefix("/client-auth/requests/") {
            if let Some((id, action)) = tail.split_once('/') {
                if method == "POST" && matches!(action, "approve" | "reject") {
                    let code = serde_json::from_slice::<Value>(body)
                        .ok()
                        .and_then(|v| v["code"].as_str().map(str::to_owned))
                        .unwrap_or_default();
                    let result = store::Store::open(&self.path)?.decide(
                        id,
                        &code,
                        action == "approve",
                        principal.actor(),
                    );
                    return Ok(match result {
                        Ok(record) => Gate::Response(200, json!(record)),
                        Err(error) => Gate::Response(409, json!({"error":error})),
                    });
                }
            }
        }
        if let Some(id) = path
            .strip_prefix("/client-auth/clients/")
            .filter(|id| !id.is_empty() && !id.contains('/'))
        {
            if method == "DELETE" {
                return Ok(
                    match store::Store::open(&self.path)?.revoke(id, principal.actor()) {
                        Ok(()) => Gate::Response(200, json!({"revoked":true})),
                        Err(error) => Gate::Response(409, json!({"error":error})),
                    },
                );
            }
        }
        if path.starts_with("/client-auth") {
            return Ok(Gate::Response(404, json!({"error":"客户端授权接口不存在"})));
        }
        Ok(Gate::Allow(principal))
    }
}

pub fn cli(args: &[String]) -> Option<Result<(), String>> {
    if args.first().map(String::as_str) != Some("client-auth") {
        return None;
    }
    Some((|| {
        let mut store = store::Store::open(&crate::state_path())?;
        match args.get(1).map(String::as_str) {
            Some("list") if args.len()==2 => println!("{}",json!({"pending":store.list(true)?,"clients":store.list(false)?})),
            Some(action @ ("approve"|"reject")) if args.len()==4 => println!("{}",json!(store.decide(&args[2],&args[3],action=="approve","local")?)),
            Some("revoke") if args.len()==3 => { store.revoke(&args[2],"local")?; println!("revoked"); }
            _ => return Err("用法：omnistream client-auth list | approve <request-id> <code> | reject <request-id> <code> | revoke <client-id>".into()),
        }
        Ok(())
    })())
}
