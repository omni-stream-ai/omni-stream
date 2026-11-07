#[path = "../src/mock_model.rs"]
mod mock_model;
use serde_json::json;
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "workflow timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn interactive_app_server_approval_and_follow_up_restore_thread() {
    let home = tempfile::tempdir().unwrap();
    let decision = json!({"kind":"delegate","project":home.path(),"reason":"inspect"});
    let steps =
        serde_json::from_value(json!([{"body":{"output_text":decision.to_string()}}])).unwrap();
    let server = mock_model::MockServer::start("127.0.0.1:0", steps).unwrap();
    let script = home.path().join("server.sh");
    std::fs::write(&script, r#"
read initialize
echo '{"id":1,"result":{}}'
read initialized
read thread
if test -f seen; then
  case "$thread" in *'"method":"thread/resume"'*) ;; *) exit 1;; esac
else
  case "$thread" in *'"method":"thread/start"'*) ;; *) exit 2;; esac
fi
echo '{"id":2,"result":{"thread":{"id":"persistent-thread"}}}'
read turn
echo '{"id":3,"result":{"turn":{"id":"turn-1"}}}'
echo '{"id":"web-read","method":"item/commandExecution/requestApproval","params":{"threadId":"persistent-thread","turnId":"turn-1","command":"curl -q https://example.com"}}'
read web_decision
case "$web_decision" in *'"decision":"accept"'*) ;; *) exit 4;; esac
echo '{"id":"approve","method":"item/commandExecution/requestApproval","params":{"threadId":"persistent-thread","turnId":"turn-1","command":"inspect"}}'
read decision
case "$decision" in *'"decision":"accept"'*) ;; *) exit 3;; esac
touch seen
echo '{"method":"item/completed","params":{"threadId":"persistent-thread","turnId":"turn-1","item":{"type":"agentMessage","text":"fixture-finished"}}}'
echo '{"method":"turn/completed","params":{"threadId":"persistent-thread","turn":{"id":"turn-1","status":"completed"}}}'
"#).unwrap();
    let config = format!(
        r#"
model_provider = "mock"
model = "mock"
[model_providers.mock]
base_url = "{}"
[model_providers.mock.auth]
command = "printf"
args = ["test"]
[agents.fixture]
command = "sh"
protocol = "app-server"
args = ["{}"]
timeout_ms = 30
"#,
        server.url,
        script.display()
    );
    std::fs::write(home.path().join("config.toml"), config).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_omnistream"))
        .env("OMNISTREAM_HOME", home.path())
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "inspect project\nfixture").unwrap();
    let dbpath = home.path().join("omnistream.db");
    wait_for(|| dbpath.exists());
    let db = rusqlite::Connection::open(dbpath).unwrap();
    db.busy_timeout(Duration::from_secs(1)).unwrap();
    for turn in 1..=2 {
        wait_for(|| {
            db.query_row(
                "SELECT COUNT(*) FROM approvals WHERE status='pending'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
                > 0
        });
        let id: i64 = db
            .query_row("SELECT id FROM approvals WHERE status='pending'", [], |r| {
                r.get(0)
            })
            .unwrap();
        // Legacy Agent timeout must not expire an approval or its pending turn.
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            db.query_row("SELECT status FROM approvals WHERE id=?1", [id], |r| r
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "pending"
        );
        writeln!(input, "approval approve {id}").unwrap();
        wait_for(|| {
            let history: String = db
                .query_row("SELECT data FROM conversations", [], |r| r.get(0))
                .unwrap_or_default();
            history.matches("fixture-finished").count() >= turn
        });
        if turn == 1 {
            // The persisted result is written just before the worker clears its busy flag.
            std::thread::sleep(Duration::from_millis(30));
            writeln!(input, "agent continue explain more").unwrap();
        }
    }
    writeln!(input, "exit").unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM agent_sessions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(
        db.query_row(
            "SELECT COUNT(*) FROM events WHERE kind='agent.event'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap()
            > 0
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}
