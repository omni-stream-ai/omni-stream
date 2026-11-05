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
fn start(home: &std::path::Path) -> (Server, reqwest::blocking::Client, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_omnistream"))
            .args(["serve", &addr.to_string()])
            .env("OMNISTREAM_HOME", home)
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
#[test]
fn scheduled_delegation_delivers_to_chat_without_creating_development_tasks_and_survives_restart() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "[agents.default]\ncommand='fixture'\nprotocol='mock'\n",
    )
    .unwrap();
    let (server, client, base) = start(home.path());
    let created = post(
        &client,
        &base,
        "/automations",
        json!({"title":"Daily digest","goal":"Collect sources","schedule":{"kind":"interval","seconds":60},"conversation":"inbox"}),
    );
    assert_eq!(created["status"], "active");
    assert!(created["project"].is_null());
    post(&client, &base, "/automations/1/run", json!({}));
    let item = wait_run(&client, &base, "succeeded");
    assert_eq!(item["status"], "active");
    assert_eq!(item["runs"].as_array().unwrap().len(), 1);
    let detail = post(
        &client,
        &base,
        "/conversation/detail",
        json!({"conversation":"inbox"}),
    );
    assert_eq!(detail["history"].as_array().unwrap().len(), 1);
    assert_eq!(detail["history"][0]["automation_id"], 1);
    assert_eq!(
        client.get(format!("{base}/tasks")).send().unwrap().status(),
        404
    );
    post(&client, &base, "/automations/1/pause", json!({}));
    drop(server);
    let (_server, client, base) = start(home.path());
    let item = items(&client, &base).remove(0);
    assert_eq!(item["status"], "paused");
    assert_eq!(item["runs"].as_array().unwrap().len(), 1);
    assert!(item["next_run_at"].is_null());
    post(&client, &base, "/automations/1/resume", json!({}));
    let edited = post(
        &client,
        &base,
        "/automations/1/update",
        json!({"title":"Changed","goal":"New sources","schedule":{"kind":"daily","hour":9,"minute":0,"utc_offset_minutes":480}}),
    );
    assert_eq!(edited["conversation"], "inbox");
    assert_eq!(edited["runs"].as_array().unwrap().len(), 1);
    post(&client, &base, "/automations/1/cancel", json!({}));
    assert!(client
        .post(format!("{base}/automations/1/resume"))
        .json(&json!({}))
        .send()
        .unwrap()
        .status()
        .is_client_error());
}
#[test]
fn missing_agent_is_a_failed_run_delivered_to_generated_inbox() {
    let home = tempfile::tempdir().unwrap();
    let (_server, client, base) = start(home.path());
    assert!(client
        .post(format!("{base}/automations"))
        .json(&json!({"title":"invalid","goal":"missing schedule"}))
        .send()
        .unwrap()
        .status()
        .is_client_error());
    post(
        &client,
        &base,
        "/automations",
        json!({"title":"Research","goal":"Collect sources","schedule":{"kind":"interval","seconds":60}}),
    );
    post(&client, &base, "/automations/1/run", json!({}));
    let item = wait_run(&client, &base, "failed");
    assert!(item["runs"][0]["output"]
        .as_str()
        .unwrap()
        .contains("未配置执行 Agent"));
    assert_eq!(item["conversation"], "automation:1");
    assert_eq!(item["status"], "active");
    let detail = post(
        &client,
        &base,
        "/conversation/detail",
        json!({"conversation":"automation:1"}),
    );
    assert_eq!(detail["history"].as_array().unwrap().len(), 1);
}

#[test]
fn pending_confirmation_survives_restart_and_executes_only_saved_user_revision() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "[agents.default]\ncommand='fixture'\nprotocol='mock'\n",
    )
    .unwrap();
    let (server, client, base) = start(home.path());
    post(
        &client,
        &base,
        "/automations",
        json!({"title":"Needs approval","goal":"Original instructions","schedule":{"kind":"interval","seconds":3600},"confirmation":{"before_seconds":600},"time_window":{"kind":"lookback","seconds":60},"conversation":"approval-inbox"}),
    );
    post(&client, &base, "/automations/1/run", json!({}));
    let deadline = Instant::now() + Duration::from_secs(10);
    let review = loop {
        let item = items(&client, &base).remove(0);
        assert!(item["runs"].as_array().unwrap().is_empty());
        if let Some(review) = item["confirmations"].as_array().unwrap().first() {
            break review.clone();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(30));
    };
    let original_next = items(&client, &base)[0]["next_run_at"].clone();
    drop(server);
    let (_server, client, base) = start(home.path());
    let item = items(&client, &base).remove(0);
    assert_eq!(item["confirmations"][0]["id"], review["id"]);
    assert_eq!(item["confirmations"][0]["status"], "pending");
    let detail = post(
        &client,
        &base,
        "/conversation/detail",
        json!({"conversation":"approval-inbox"}),
    );
    assert_eq!(detail["history"].as_array().unwrap().len(), 1);
    let changed = post(
        &client,
        &base,
        "/automations/1/confirmations/1/update",
        json!({"revision":1,"goal":"Approved custom instructions"}),
    );
    assert_eq!(changed["revision"], 2);
    assert!(items(&client, &base)[0]["runs"]
        .as_array()
        .unwrap()
        .is_empty());
    post(
        &client,
        &base,
        "/automations/1/confirmations/1/approve",
        json!({"revision":2}),
    );
    let item = wait_run(&client, &base, "succeeded");
    assert_eq!(item["goal"], "Original instructions");
    assert_eq!(
        item["runs"][0]["context"]["goal"],
        "Approved custom instructions"
    );
    assert_eq!(item["runs"][0]["scheduled_at"], review["scheduled_at"]);
    assert_eq!(
        item["runs"][0]["context"]["time_window"]["end"],
        review["scheduled_at"]
    );
    assert_eq!(item["next_run_at"], original_next);
    assert_eq!(item["confirmations"][0]["status"], "consumed");
}

#[test]
fn early_approval_persists_and_configuration_changes_invalidate_it() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "[agents.default]\ncommand='fixture'\nprotocol='mock'\n",
    )
    .unwrap();
    let (server, client, base) = start(home.path());
    let draft = json!({"title":"Early review","goal":"Instructions","schedule":{"kind":"interval","seconds":3600},"confirmation":{"before_seconds":4000},"conversation":"early-inbox"});
    post(&client, &base, "/automations", draft.clone());
    let deadline = Instant::now() + Duration::from_secs(10);
    while items(&client, &base)[0]["confirmations"]
        .as_array()
        .unwrap()
        .is_empty()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(30));
    }
    post(
        &client,
        &base,
        "/automations/1/confirmations/1/approve",
        json!({"revision":1}),
    );
    drop(server);
    let (_server, client, base) = start(home.path());
    let item = items(&client, &base).remove(0);
    assert_eq!(item["confirmations"][0]["status"], "approved");
    assert!(item["runs"].as_array().unwrap().is_empty());
    post(
        &client,
        &base,
        "/automations/1/calendar",
        json!({"calendar":{"weekdays":[],"include_dates":[],"exclude_dates":[]},"reason":"New calendar"}),
    );
    assert_eq!(
        items(&client, &base)[0]["confirmations"][0]["status"],
        "invalidated"
    );
    let response = client
        .post(format!("{base}/automations/1/confirmations/1/approve"))
        .json(&json!({"revision":1}))
        .send()
        .unwrap();
    assert!(!response.status().is_success());
    post(&client, &base, "/automations/1/update", draft);
    assert!(items(&client, &base)[0]["runs"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(items(&client, &base)[0]["confirmations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["id"] == 1)
        .all(|r| r["status"] == "invalidated"));
}

#[test]
fn unconfirmed_run_policy_survives_restart_with_its_original_goal_and_source() {
    for already_ready in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let agent = home.path().join("business-agent.py");
        std::fs::write(
            &agent,
            r#"from pathlib import Path
import sys
Path(__file__).with_name('business-prompt.txt').write_text(sys.argv[-1])
print('Automatic execution completed')
"#,
        )
        .unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            format!(
                "[agents.default]
command='python3'
protocol='cli'
args=[{:?}]
",
                agent.to_string_lossy()
            ),
        )
        .unwrap();
        let (server, client, base) = start(home.path());
        post(
            &client,
            &base,
            "/automations",
            json!({"title":"Run if unconfirmed","goal":"Original authorized instructions","conversation":"timeout-inbox","schedule":{"kind":"interval","seconds":3600},"confirmation":{"before_seconds":4000,"on_timeout":"run"}}),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while items(&client, &base)[0]["confirmations"]
            .as_array()
            .unwrap()
            .is_empty()
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        post(
            &client,
            &base,
            "/automations/1/confirmations/1/update",
            json!({"revision":1,"goal":"Unapproved proposal must not execute"}),
        );
        drop(server);
        // Simulate the clock reaching the occurrence while this isolated Core is offline.
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 1;
        {
            let db = rusqlite::Connection::open(home.path().join("omnistream.db")).unwrap();
            let text: String = db
                .query_row(
                    "SELECT data FROM app_settings WHERE key='automations'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let mut state: Value = serde_json::from_str(&text).unwrap();
            state["items"][0]["next_run_at"] = json!(at);
            let review = &mut state["items"][0]["confirmations"][0];
            review["scheduled_at"] = json!(at);
            review["deadline"] = json!(at);
            review["discussion_active"] = json!(true);
            if already_ready {
                review["status"] = json!("ready");
                review["execution_source"] = json!("timeout");
                review["execution_goal"] = json!("Original authorized instructions");
                review["decided_at"] = json!(at);
            }
            db.execute(
                "UPDATE app_settings SET data=?1 WHERE key='automations'",
                [state.to_string()],
            )
            .unwrap();
        }
        let (server, client, base) = start(home.path());
        let item = wait_run(&client, &base, "succeeded");
        assert_eq!(item["runs"].as_array().unwrap().len(), 1);
        assert_eq!(item["runs"][0]["scheduled_at"], at);
        assert_eq!(
            item["runs"][0]["context"]["goal"],
            "Original authorized instructions"
        );
        assert_eq!(
            item["runs"][0]["context"]["confirmation"]["execution_source"],
            "timeout"
        );
        assert_eq!(item["confirmations"][0]["status"], "consumed");
        let prompt = std::fs::read_to_string(home.path().join("business-prompt.txt")).unwrap();
        assert!(prompt.contains("Original authorized instructions"));
        assert!(!prompt.contains("Unapproved proposal must not execute"));
        assert!(item["confirmations"][0]["approved_at"].is_null());
        assert_eq!(
            item["confirmations"][0]["goal"],
            "Unapproved proposal must not execute"
        );
        drop(server);
        let (_server, client, base) = start(home.path());
        assert_eq!(
            items(&client, &base)[0]["runs"].as_array().unwrap().len(),
            1
        );
    }
}
