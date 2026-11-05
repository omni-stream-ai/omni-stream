use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Core(Child);
impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_omnistream"));
    command.env("OMNISTREAM_HOME", home).current_dir(home);
    command
}

#[test]
fn removed_development_flows_leave_old_data_intact_and_agent_calls_work() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "[agents.default]\ncommand='unused'\nprotocol='mock'\n",
    )
    .unwrap();
    // Initialize the schema, then seed a running development task from an old Core.
    assert!(command(home.path())
        .arg("status")
        .output()
        .unwrap()
        .status
        .success());
    let db = Connection::open(home.path().join("omnistream.db")).unwrap();
    let legacy = json!({"id":41,"project":home.path(),"title":"Old development task","goal":"Old goal","status":"running","plan":["Old plan"],"approved":true,"transient":false}).to_string();
    db.execute("INSERT INTO tasks(id,data) VALUES (41,?1)", params![legacy])
        .unwrap();
    db.execute("INSERT INTO conversation_calls_v2(task_id,conversation,scope,agent) VALUES (41,'current','old-scope','default')", []).unwrap();
    let output = command(home.path())
        .args(["task", "create", "must not create a task"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("旧开发任务命令已移除"));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let _core = Core(
        command(home.path())
            .args(["serve", &address.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let base = format!("http://{address}");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if client.get(format!("{base}/health")).send().is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "Core startup timed out");
        std::thread::sleep(Duration::from_millis(30));
    }
    for path in [
        "/tasks",
        "/tasks/41/events",
        "/tasks/41/conversations",
        "/recordings/old.mp4",
    ] {
        assert_eq!(
            client.get(format!("{base}{path}")).send().unwrap().status(),
            404,
            "{path}"
        );
    }
    for path in [
        "/tasks",
        "/tasks/41/run",
        "/tasks/41/approve",
        "/tasks/41/preparation",
        "/tasks/41/plan",
        "/tasks/41/cancel",
        "/task-proposals/confirm",
    ] {
        assert_eq!(
            client
                .post(format!("{base}{path}"))
                .json(&json!({}))
                .send()
                .unwrap()
                .status(),
            404,
            "{path}"
        );
    }
    assert!(!client
        .post(format!("{base}/agent-calls/41/cancel"))
        .send()
        .unwrap()
        .status()
        .is_success());
    assert_eq!(
        db.query_row("SELECT data FROM tasks WHERE id=41", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        legacy
    );

    let agent: Value = client.post(format!("{base}/agents"))
        .json(&json!({"name":"test-agent","description":"Test","instructions":"Reply","engine":"default","plugins":[]}))
        .send().unwrap().error_for_status().unwrap().json().unwrap();
    client.post(format!("{base}/chat/stream"))
        .json(&json!({"conversation":"current","agent_id":agent["id"],"messages":[{"role":"user","content":"hello"}]}))
        .send().unwrap().error_for_status().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let detail: Value = client
            .post(format!("{base}/conversation/detail"))
            .json(&json!({"conversation":"current"}))
            .send()
            .unwrap()
            .json()
            .unwrap();
        if detail["calls"][0]["task"]["status"] == "unverified" {
            assert_eq!(detail["calls"][0]["task_id"], 42);
            assert_eq!(detail["calls"].as_array().unwrap().len(), 1);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Agent call did not finish: {detail}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(
        db.query_row("SELECT data FROM tasks WHERE id=41", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        legacy
    );
}
