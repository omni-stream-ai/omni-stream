use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Core {
    child: Child,
    base: String,
    client: Client,
}
impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Core {
    fn start(home: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let child = Command::new(env!("CARGO_BIN_EXE_omnistream"))
            .args(["serve", &address.to_string()])
            .current_dir(home)
            .env("OMNISTREAM_HOME", home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let core = Self {
            child,
            base: format!("http://{address}"),
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !core
            .client
            .get(format!("{}/health", core.base))
            .send()
            .is_ok_and(|r| r.status().is_success())
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(25));
        }
        core
    }
    fn post(&self, path: &str, body: &Value) -> reqwest::blocking::Response {
        self.client
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .unwrap()
    }
    fn install(&self, id: &str) {
        assert!(self
            .post(&format!("/plugins/{id}/install"), &json!({}))
            .status()
            .is_success());
    }
    fn detail(&self, conversation: &str) -> Value {
        self.post(
            "/conversation/detail",
            &json!({"conversation":conversation}),
        )
        .json()
        .unwrap()
    }
    fn finish(&self, conversation: &str, count: usize) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let detail = self.detail(conversation);
            if detail["calls"].as_array().is_some_and(|calls| {
                calls.len() == count
                    && calls
                        .iter()
                        .all(|call| call["task"]["status"] == "unverified")
            }) {
                return detail;
            }
            assert!(Instant::now() < deadline, "Agent did not finish: {detail}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
fn options() -> Value {
    json!({"auth_mode":"token","listen":false,"listen_scope":"related","allow_writes":false,"enabled":true,"locale":"zh-CN"})
}
fn request(n: usize, options: Value) -> Value {
    json!({"request_id":format!("00000000-0000-0000-0000-{n:012}"),"options":options})
}

#[test]
fn plugin_setup_missing_agent_and_secret_payloads_fail_without_creating_a_conversation() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "").unwrap();
    let core = Core::start(home.path());
    assert_eq!(
        core.post("/plugins/unknown/setup", &request(0, options()))
            .status()
            .as_u16(),
        404
    );
    assert_eq!(
        core.post("/plugins/slack/setup", &request(0, options()))
            .status()
            .as_u16(),
        409
    );
    core.install("slack");
    assert_eq!(
        core.client
            .get(format!("{}/plugins/slack/setup", core.base))
            .send()
            .unwrap()
            .status()
            .as_u16(),
        405
    );
    for nested in [false, true] {
        let mut body = request(0, options());
        if nested {
            body["options"]["token"] = json!("private-fixture-token");
        } else {
            body["app_token"] = json!("private-fixture-token");
        }
        let response = core.post("/plugins/slack/setup", &body);
        assert_eq!(response.status().as_u16(), 400);
        assert!(!response.text().unwrap().contains("private-fixture-token"));
    }
    let response = core.post("/plugins/slack/setup", &request(0, options()));
    assert_eq!(response.status().as_u16(), 409);
    assert!(response.text().unwrap().contains("config.toml"));
    let conversations: Value = core
        .client
        .get(format!("{}/conversations", core.base))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(conversations, json!([]));
}

#[test]
fn plugin_setup_starts_real_agents_and_persists_options_idempotency_and_continuation_without_a_model(
) {
    let home = tempfile::tempdir().unwrap();
    let script = home.path().join("setup-agent.sh");
    let log = home.path().join("agent-prompts.txt");
    std::fs::write(
        &script,
        r#"printf '%s\n' "$2" >> "$1"
case "$2" in *pause_for_test*) sleep 0.5 ;; esac
i=0
while [ "$i" -lt 200 ]; do printf 'Setup progress. '; i=$((i+1)); done
printf 'xoxp-private-fixture ghp_private-fixture ntn_private-fixture figd_private-fixture'
"#,
    )
    .unwrap();
    std::fs::write(home.path().join("config.toml"), format!("[agents.a]\ncommand=\"sh\"\nprotocol=\"cli\"\nargs=[{:?},{:?}]\n[agents.z]\ncommand=\"unused\"\nprotocol=\"mock\"\n", script.to_str().unwrap(), log.to_str().unwrap())).unwrap();
    let core = Core::start(home.path());
    let mut saved = Vec::new();
    for (n, id) in ["slack", "github", "notion", "figma"].iter().enumerate() {
        core.install(id);
        let mut selected = options();
        if *id == "slack" {
            selected["auth_mode"] = json!("slack_user");
            selected["listen"] = json!(true);
            selected["listen_scope"] = json!("all");
            selected["allow_writes"] = json!(true);
        }
        let body = request(n, selected.clone());
        let response = core.post(&format!("/plugins/{id}/setup"), &body);
        assert!(
            response.status().is_success(),
            "{}",
            response.text().unwrap()
        );
        // Repeat after starting: the same request opens the same conversation and call.
        let started: Value = core
            .post(&format!("/plugins/{id}/setup"), &body)
            .json()
            .unwrap();
        let conversation = started["conversation"].as_str().unwrap();
        let detail = core.finish(conversation, 1);
        assert_eq!(detail["settings"]["approval_mode"], "default");
        assert_eq!(detail["history"][0]["plugin_setup"]["options"], selected);
        assert_eq!(detail["calls"][0]["agent"], "a");
        assert_eq!(detail["calls"][0]["task_id"], started["call_id"]);
        assert!(detail.to_string().contains("[redacted]"));
        assert!(!detail.to_string().contains("private-fixture"));
        saved.push((id.to_string(), body, started));
    }
    assert_eq!(
        core.client
            .get(format!("{}/tasks", core.base))
            .send()
            .unwrap()
            .status(),
        404
    );
    assert!(!home.path().join("plugins/connections/slack.json").exists());
    let recorded = std::fs::read_to_string(&log).unwrap();
    assert!(recorded.contains("User Token Scopes"));
    assert!(recorded.contains("监听范围：所有已加入"));
    assert!(recorded.contains("connections:write"));
    assert!(!recorded.contains("private-fixture"));

    let conversation = saved[0].2["conversation"].as_str().unwrap();
    let send = |text: &str| json!({"conversation":conversation,"project":"/nonexistent-project","messages":[{"role":"user","content":text}]});
    assert!(core
        .post(
            "/chat/stream",
            &send("pause_for_test 已完成登录，接下来做什么？")
        )
        .status()
        .is_success());
    assert_eq!(
        core.post("/chat/stream", &send("must_not_be_queued"))
            .status()
            .as_u16(),
        502
    );
    let detail = core.finish(conversation, 2);
    assert!(detail["history"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["content"]
            .as_str()
            .is_some_and(|s| s.contains("已完成登录"))));
    assert!(!std::fs::read_to_string(&log)
        .unwrap()
        .contains("must_not_be_queued"));
    assert_eq!(
        core.post("/chat/stream", &send("xoxp-do-not-store-this"))
            .status()
            .as_u16(),
        502
    );
    let mut with_attachment = send("请看截图");
    with_attachment["attachments"] = json!([{"data":"private-image"}]);
    assert_eq!(
        core.post("/chat/stream", &with_attachment)
            .status()
            .as_u16(),
        502
    );
    assert!(!core
        .detail(conversation)
        .to_string()
        .contains("do-not-store-this"));
    assert!(!home.path().join("attachments").exists());
    let old_scope = detail["calls"][0]["scope"].clone();
    drop(core);
    let restarted = Core::start(home.path());
    let repeated: Value = restarted
        .post("/plugins/slack/setup", &saved[0].1)
        .json()
        .unwrap();
    assert_eq!(repeated, saved[0].2);
    assert!(restarted
        .post("/chat/stream", &send("继续检查配置"))
        .status()
        .is_success());
    let detail = restarted.finish(conversation, 3);
    assert_eq!(detail["calls"][2]["scope"], old_scope);
    assert_eq!(detail["history"][0]["plugin_setup"]["plugin"], "slack");
}
