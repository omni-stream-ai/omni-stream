use super::{
    credentials::{self, Credentials},
    now, random,
    storage::Device,
    Settings,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{io::Write, path::Path, time::Duration};
use web_push::{ContentEncoding, SubscriptionInfo, VapidSignatureBuilder, WebPushMessageBuilder};

pub enum Outcome {
    Delivered,
    Invalid,
    Failed(&'static str, bool),
}

pub fn vapid(db: &Path) -> Result<(String, String), String> {
    let path = db.with_file_name("push-vapid.key");
    if !path.exists() {
        let key = loop {
            let key = random()?;
            if VapidSignatureBuilder::from_base64_no_sub(&key).is_ok() {
                break key;
            }
        };
        let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("缺少推送密钥目录")?)
            .map_err(|_| "无法创建推送密钥")?;
        file.write_all(key.as_bytes())
            .map_err(|_| "无法保存推送密钥")?;
        file.as_file().sync_all().map_err(|_| "无法保存推送密钥")?;
        if let Err(e) = file.persist_noclobber(&path) {
            if e.error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err("无法保存推送密钥".into());
            }
        }
    }
    let key = std::fs::read_to_string(path).map_err(|_| "无法读取 Web Push 密钥")?;
    let builder =
        VapidSignatureBuilder::from_base64_no_sub(key.trim()).map_err(|_| "Web Push 密钥无效")?;
    Ok((
        key.trim().to_owned(),
        URL_SAFE_NO_PAD.encode(builder.get_public_key()),
    ))
}

pub fn validate_target(transport: &str, target: &Value) -> Result<String, String> {
    match transport {
        "android" => {
            let token = target
                .as_str()
                .filter(|v| {
                    v.len() >= 20 && v.len() <= 4096 && v.bytes().all(|b| b.is_ascii_graphic())
                })
                .ok_or("FCM token 无效")?;
            Ok(format!("android:{token}"))
        }
        "web" => {
            let sub: SubscriptionInfo =
                serde_json::from_value(target.clone()).map_err(|_| "Web Push 订阅无效")?;
            let url = reqwest::Url::parse(&sub.endpoint).map_err(|_| "Web Push 地址无效")?;
            let host = url.host_str().unwrap_or("");
            let trusted = host == "fcm.googleapis.com"
                || host == "updates.push.services.mozilla.com"
                || host.ends_with(".push.services.mozilla.com")
                || host == "web.push.apple.com"
                || host.ends_with(".notify.windows.com");
            if url.scheme() != "https"
                || url.port_or_known_default() != Some(443)
                || !trusted
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
                || sub.endpoint.len() > 4096
            {
                return Err("不支持的 Web Push 服务地址".into());
            }
            if URL_SAFE_NO_PAD
                .decode(&sub.keys.p256dh)
                .map_or(true, |k| k.len() != 65 || k[0] != 4)
                || URL_SAFE_NO_PAD
                    .decode(&sub.keys.auth)
                    .map_or(true, |k| k.len() != 16)
            {
                return Err("Web Push 订阅密钥无效".into());
            }
            Ok(format!("web:{}", sub.endpoint))
        }
        _ => Err("当前仅支持 Android 和浏览器远程推送".into()),
    }
}

pub fn fcm_ready(settings: &Settings) -> bool {
    credentials::load(settings).is_ok()
}

pub fn payload(device: &Device, instance: &str, item: &Value) -> Value {
    let english = device.locale == "en-US";
    let title = match (item["kind"].as_str().unwrap_or(""), english) {
        ("task_completed", false) => "任务执行完成",
        ("task_completed", true) => "Task execution finished",
        ("task_failed", false) => "任务失败或中断",
        ("task_failed", true) => "Task failed or interrupted",
        ("approval_pending", false) => "需要你的响应",
        ("approval_pending", true) => "Your response is needed",
        (_, false) => "Slack 收件箱有新消息",
        (_, true) => "New Slack inbox message",
    };
    let encode = |s: &str| -> String {
        s.bytes()
            .map(|c| {
                if c.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&c) {
                    (c as char).to_string()
                } else {
                    format!("%{c:02X}")
                }
            })
            .collect()
    };
    let path = if let Some(channel) = item["channel"].as_str() {
        format!("/slack?channel={}", encode(channel))
    } else {
        format!(
            "/chat/{}",
            encode(item["conversation"].as_str().unwrap_or(""))
        )
    };
    json!({"title":title,"body":item["title"],"path":path,"server":device.server,"device":device.id,
        "scope":crate::notifications::scope(item),
        "tag":format!("omnistream-{instance}-{}",item["id"]),"instance":instance,"id":item["id"].to_string()})
}

pub struct Sender {
    client: Client,
    fcm: Option<Credentials>,
    vapid: String,
    access: Option<(String, i64)>,
}
impl Sender {
    pub fn new(settings: Settings, vapid: String) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "无法初始化推送服务")?;
        Ok(Self {
            client,
            fcm: credentials::load(&settings).ok(),
            vapid,
            access: None,
        })
    }
    fn access_token(&mut self) -> Result<String, ()> {
        if let Some((token, expires)) = &self.access {
            if *expires > now() + 60 {
                return Ok(token.clone());
            }
        }
        let credentials = self.fcm.as_ref().ok_or(())?;
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(credentials.private_key.as_bytes())
            .map_err(|_| ())?;
        let assertion = jsonwebtoken::encode(&jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256), &json!({
            "iss":credentials.client_email,"scope":"https://www.googleapis.com/auth/firebase.messaging",
            "aud":"https://oauth2.googleapis.com/token","iat":now(),"exp":now()+3600}), &key).map_err(|_| ())?;
        let response: Value = self
            .client
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .map_err(|_| ())?
            .error_for_status()
            .map_err(|_| ())?
            .json()
            .map_err(|_| ())?;
        let token = response["access_token"].as_str().ok_or(())?.to_owned();
        self.access = Some((
            token.clone(),
            now() + response["expires_in"].as_i64().unwrap_or(3600),
        ));
        Ok(token)
    }
    pub fn send(&mut self, device: &Device, payload: &Value) -> Outcome {
        if validate_target(&device.transport, &device.target).is_err() {
            return Outcome::Invalid;
        }
        if device.transport == "web" {
            return self.web(device, payload);
        }
        let access = match self.access_token() {
            Ok(value) => value,
            Err(_) => return Outcome::Failed("authentication_failed", false),
        };
        let project = &self
            .fcm
            .as_ref()
            .expect("FCM token requires credentials")
            .project_id;
        // Data-only messages let Android discard receipts read while a push was in flight.
        let request = json!({"message":{"token":device.target,"data":payload,"android":{"priority":"HIGH","ttl":"86400s"}}});
        let response = match self
            .client
            .post(format!(
                "https://fcm.googleapis.com/v1/projects/{project}/messages:send"
            ))
            .bearer_auth(access)
            .json(&request)
            .send()
        {
            Ok(response) => response,
            Err(_) => return Outcome::Failed("delivery_unknown", true),
        };
        if response.status().is_success() {
            return Outcome::Delivered;
        }
        if response.status().is_server_error() {
            return Outcome::Failed("delivery_unknown", true);
        }
        if response.status().as_u16() == 401 {
            self.access = None;
        }
        let body: Value = response.json().unwrap_or(Value::Null);
        if body["error"]["details"]
            .as_array()
            .is_some_and(|details| details.iter().any(|d| d["errorCode"] == "UNREGISTERED"))
        {
            Outcome::Invalid
        } else {
            Outcome::Failed("provider_rejected", false)
        }
    }
    fn web(&self, device: &Device, payload: &Value) -> Outcome {
        let build = (|| {
            let sub: SubscriptionInfo =
                serde_json::from_value(device.target.clone()).map_err(|_| ())?;
            let mut signature =
                VapidSignatureBuilder::from_base64(&self.vapid, &sub).map_err(|_| ())?;
            signature.add_claim("sub", "mailto:push@omnistreamai.com");
            let mut builder = WebPushMessageBuilder::new(&sub);
            let bytes = serde_json::to_vec(payload).map_err(|_| ())?;
            builder.set_ttl(86400);
            builder.set_payload(ContentEncoding::Aes128Gcm, &bytes);
            builder.set_vapid_signature(signature.build().map_err(|_| ())?);
            Ok::<_, ()>(web_push::request_builder::build_request::<Vec<u8>>(
                builder.build().map_err(|_| ())?,
            ))
        })();
        let request = match build {
            Ok(request) => request,
            Err(_) => return Outcome::Failed("encryption_failed", false),
        };
        let (parts, body) = request.into_parts();
        let mut request = self.client.post(parts.uri.to_string());
        for (name, value) in &parts.headers {
            request = request.header(name.as_str(), value.as_bytes());
        }
        match request.body(body).send() {
            Ok(response) if response.status().is_success() => Outcome::Delivered,
            Ok(response) if [404, 410].contains(&response.status().as_u16()) => Outcome::Invalid,
            Ok(response) if response.status().is_server_error() => {
                Outcome::Failed("delivery_unknown", true)
            }
            Ok(_) => Outcome::Failed("provider_rejected", false),
            Err(_) => Outcome::Failed("delivery_unknown", true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subscriptions_reject_private_hosts_redirects_credentials_and_invalid_keys() {
        let dir = tempfile::tempdir().unwrap();
        let (_, public) = vapid(&dir.path().join("state.db")).unwrap();
        let mut target = json!({"endpoint":"https://fcm.googleapis.com/fcm/send/test","keys":{"auth":URL_SAFE_NO_PAD.encode([7;16]),"p256dh":public}});
        assert!(validate_target("web", &target).is_ok());
        for endpoint in [
            "http://fcm.googleapis.com/test",
            "https://127.0.0.1/test",
            "https://fcm.googleapis.com.attacker.example/test",
            "https://user:pass@fcm.googleapis.com/test",
            "https://fcm.googleapis.com:8443/test",
        ] {
            target["endpoint"] = json!(endpoint);
            assert!(validate_target("web", &target).is_err());
        }
        target["endpoint"] = json!("https://updates.push.services.mozilla.com/wpush/v2/test");
        target["keys"]["auth"] = json!("invalid");
        assert!(validate_target("web", &target).is_err());
        assert!(validate_target("ios", &json!("a-token")).is_err());
    }
    #[test]
    fn vapid_keys_are_stable_private_and_match_browser_public_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let (private, public) = vapid(&path).unwrap();
        assert_eq!(vapid(&path).unwrap(), (private.clone(), public.clone()));
        assert_eq!(URL_SAFE_NO_PAD.decode(&public).unwrap().len(), 65);
        assert_eq!(URL_SAFE_NO_PAD.decode(private).unwrap().len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.path().join("push-vapid.key"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
