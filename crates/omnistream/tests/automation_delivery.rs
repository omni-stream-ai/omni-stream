use serde_json::{json, Value};
use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn start(home: &std::path::Path, origin: &str) -> (Server, reqwest::blocking::Client, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_omnistream"))
            .args(["serve", &addr.to_string()])
            .env("OMNISTREAM_HOME", home)
            .env("OMNISTREAM_PLUGIN_TEST_ORIGIN", origin)
            .current_dir(home)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let base = format!("http://{addr}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while client.get(format!("{base}/health")).send().is_err() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    (server, client, base)
}
fn post(client: &reqwest::blocking::Client, base: &str, path: &str, value: Value) -> Value {
    client
        .post(format!("{base}{path}"))
        .json(&value)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap()
}
fn items(client: &reqwest::blocking::Client, base: &str) -> Vec<Value> {
    client
        .get(format!("{base}/automations"))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap()
}
fn wait_run(client: &reqwest::blocking::Client, base: &str, status: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let item = items(client, base).remove(0);
        if item["runs"][0]["status"] == status {
            return item;
        }
        assert!(Instant::now() < deadline, "{item}");
        std::thread::sleep(Duration::from_millis(40));
    }
}

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
struct Service {
    origin: String,
    writes: Arc<Mutex<Vec<(String, Value)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Service {
    fn new() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", server.server_addr());
        let writes = Arc::new(Mutex::new(vec![]));
        let records = writes.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                let Some(mut req) = server.recv_timeout(Duration::from_millis(20)).unwrap() else {
                    continue;
                };
                let url = req.url().to_owned();
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).unwrap();
                let input: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let identity = url.ends_with("auth.test")
                    || url == "/github/user"
                    || url == "/notion/users/me"
                    || url == "/figma/me";
                if identity {
                    req.respond(tiny_http::Response::from_string(json!({"ok":true,"user":"fixture","user_id":"U1","team_id":"T1","login":"fixture","name":"fixture","handle":"fixture"}).to_string())).unwrap();
                    continue;
                }
                let count = {
                    let mut records = records.lock().unwrap();
                    records.push((url.clone(), input.clone()));
                    records
                        .iter()
                        .filter(|(_, v)| v["channel"] == input["channel"])
                        .count()
                };
                let response = match input["channel"].as_str() {
                    Some("C_RETRY") if count == 1 => tiny_http::Response::from_string("{}")
                        .with_status_code(429)
                        .with_header(tiny_http::Header::from_bytes("Retry-After", "1").unwrap()),
                    Some("C_UNKNOWN") => {
                        tiny_http::Response::from_string("response lost after write")
                    }
                    Some("C_EMPTY") => tiny_http::Response::from_string("{}"),
                    Some("C_INVALID_OK") => tiny_http::Response::from_string(
                        json!({"ok":"false","error":"invalid_auth"}).to_string(),
                    ),
                    Some("C_NO_RECEIPT") => tiny_http::Response::from_string(
                        json!({"ok":true,"channel":input["channel"]}).to_string(),
                    ),
                    Some("C_FAIL") => tiny_http::Response::from_string(
                        json!({"ok":false,"error":"channel_not_found"}).to_string(),
                    ),
                    _ => tiny_http::Response::from_string(
                        match url.as_str() {
                            "/slack/chat.postMessage" => {
                                json!({"ok":true,"channel":input["channel"],"ts":"123.456"})
                            }
                            "/github/repos/org/repo/issues" => json!({"id":1,"number":42}),
                            "/notion/pages" => json!({"object":"page","id":"created-page"}),
                            _ => json!({"id":"created-comment"}),
                        }
                        .to_string(),
                    ),
                };
                req.respond(response).unwrap();
            }
        });
        Self {
            origin,
            writes,
            stop,
            thread: Some(thread),
        }
    }
    fn count(&self) -> usize {
        self.writes.lock().unwrap().len()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn configure(client: &reqwest::blocking::Client, base: &str, plugin: &str) {
    post(
        client,
        base,
        &format!("/plugins/{plugin}/install"),
        json!({}),
    );
    post(
        client,
        base,
        &format!("/plugins/{plugin}/connection"),
        json!({"token":"fixture-token","enabled":true,"allow_writes":true}),
    );
}
fn target(id: &str, channel: &str) -> Value {
    json!({"type":"plugin_action","id":id,"plugin":"slack","action":"send_message","destination":{"channel":channel},"payload":{"text":"{{run.output_markdown}}"},"max_attempts":3})
}
fn draft(targets: Value) -> Value {
    json!({"title":"Delivery fixture","goal":"Generate a report","schedule":{"kind":"interval","seconds":3600},"conversation":"inbox","delivery_targets":targets})
}
fn home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "[agents.default]\ncommand='fixture'\nprotocol='mock'\n",
    )
    .unwrap();
    home
}
#[test]
fn multi_application_delivery_survives_restart_without_replay() {
    let service = Service::new();
    let home = home();
    let (server, client, base) = start(home.path(), &service.origin);
    for plugin in ["slack", "github", "notion", "figma"] {
        configure(&client, &base, plugin);
    }
    let targets = json!([target("slack","C_OK"),{"type":"plugin_action","id":"github","plugin":"github","action":"create_issue","destination":{"owner":"org","repo":"repo"},"payload":{"title":"Report","body":"{{run.output_markdown}}"}},{"type":"plugin_action","id":"notion","plugin":"notion","action":"create_page","destination":{"parent_page_id":"page"},"payload":{"title":"Report","content":"{{run.output_markdown}}"}},{"type":"plugin_action","id":"figma","plugin":"figma","action":"post_comment","destination":{"file_key":"file"},"payload":{"message":"{{run.output_markdown}}"}}]);
    let created = post(&client, &base, "/automations", draft(targets));
    assert_eq!(created["delivery_grants"].as_array().unwrap().len(), 4);
    assert!(!created.to_string().contains("fixture-token"));
    post(&client, &base, "/automations/1/run", json!({}));
    let item = wait_run(&client, &base, "succeeded");
    assert_eq!(service.count(), 4);
    assert!(item["runs"][0]["deliveries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["status"] == "succeeded" && r["attempts"] == 1));
    assert!(item["runs"][0]["generation_output"].is_string());
    drop(server);
    let (_server, client, base) = start(home.path(), &service.origin);
    assert_eq!(
        items(&client, &base)[0]["runs"][0]["deliveries"],
        item["runs"][0]["deliveries"]
    );
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(service.count(), 4);
}
#[test]
fn only_definite_retryable_failures_retry_and_unknown_is_not_replayed() {
    let service = Service::new();
    let home = home();
    let (_server, client, base) = start(home.path(), &service.origin);
    configure(&client, &base, "slack");
    post(
        &client,
        &base,
        "/automations",
        draft(json!([
            target("retry", "C_RETRY"),
            target("unknown", "C_UNKNOWN"),
            target("definite", "C_FAIL")
        ])),
    );
    post(&client, &base, "/automations/1/run", json!({}));
    let item = wait_run(&client, &base, "failed");
    let receipts = item["runs"][0]["deliveries"].as_array().unwrap();
    assert_eq!(receipts[0]["status"], "succeeded");
    assert_eq!(receipts[0]["attempts"], 2);
    assert_eq!(receipts[1]["status"], "unknown");
    assert_eq!(receipts[1]["attempts"], 1);
    assert_eq!(receipts[2]["status"], "failed");
    assert_eq!(receipts[2]["attempts"], 1);
    assert_eq!(service.count(), 4);
}

#[test]
fn malformed_success_responses_are_unknown_and_never_replayed() {
    let service = Service::new();
    let home = home();
    let (server, client, base) = start(home.path(), &service.origin);
    configure(&client, &base, "slack");
    post(
        &client,
        &base,
        "/automations",
        draft(json!([
            target("empty", "C_EMPTY"),
            target("invalid-ok", "C_INVALID_OK"),
            target("missing-receipt", "C_NO_RECEIPT")
        ])),
    );
    post(&client, &base, "/automations/1/run", json!({}));
    let item = wait_run(&client, &base, "failed");
    let receipts = item["runs"][0]["deliveries"].as_array().unwrap();
    assert_eq!(receipts.len(), 3);
    for receipt in receipts {
        assert_eq!(receipt["status"], "unknown");
        assert_eq!(receipt["attempts"], 1);
    }
    assert_eq!(service.count(), 3);
    drop(server);
    let (_server, client, base) = start(home.path(), &service.origin);
    assert_eq!(
        items(&client, &base)[0]["runs"][0]["deliveries"],
        json!(receipts)
    );
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(service.count(), 3);
}
#[test]
fn revoked_grants_and_connection_changes_block_background_writes() {
    let service = Service::new();
    let home = home();
    let (_server, client, base) = start(home.path(), &service.origin);
    configure(&client, &base, "slack");
    post(
        &client,
        &base,
        "/automations",
        draft(json!([target("report", "C_OK")])),
    );
    post(&client, &base, "/automations/1/revoke", json!({}));
    post(&client, &base, "/automations/1/run", json!({}));
    let item = wait_run(&client, &base, "failed");
    assert_eq!(item["runs"][0]["deliveries"][0]["status"], "failed");
    assert_eq!(service.count(), 0);
    post(
        &client,
        &base,
        "/automations/1/update",
        draft(json!([target("report", "C_OK")])),
    );
    post(
        &client,
        &base,
        "/plugins/slack/connection",
        json!({"enabled":false,"allow_writes":false}),
    );
    post(&client, &base, "/automations/1/run", json!({}));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let item = items(&client, &base).remove(0);
        if item["runs"].as_array().unwrap().len() == 2 && item["runs"][1]["status"] == "failed" {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(service.count(), 0);
}

#[test]
fn failure_policies_record_skips_and_fallback_without_rolling_back_successes() {
    for (policy, status, writes) in [
        ("fail_fast", "failed", 1),
        ("fallback_in_app", "succeeded", 2),
    ] {
        let service = Service::new();
        let home = home();
        let (_server, client, base) = start(home.path(), &service.origin);
        configure(&client, &base, "slack");
        let mut value = draft(json!([target("first", "C_FAIL"), target("second", "C_OK")]));
        value["delivery_policy"] = json!(policy);
        post(&client, &base, "/automations", value);
        post(&client, &base, "/automations/1/run", json!({}));
        let item = wait_run(&client, &base, status);
        let receipts = item["runs"][0]["deliveries"].as_array().unwrap();
        assert_eq!(receipts[0]["status"], "failed");
        assert_eq!(
            receipts[1]["status"],
            if policy == "fail_fast" {
                "skipped"
            } else {
                "succeeded"
            }
        );
        assert_eq!(service.count(), writes);
        let conversation = post(
            &client,
            &base,
            "/conversation/detail",
            json!({"conversation":"inbox"}),
        );
        assert_eq!(conversation["history"].as_array().unwrap().len(), 1);
    }
}
