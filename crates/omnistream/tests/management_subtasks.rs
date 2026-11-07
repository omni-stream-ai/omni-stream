//! Real Core with an isolated database, scripted manager and local CLI Agent.
#[path = "../src/mock_model.rs"]
mod mock_model;
use serde_json::{json, Value};
use std::{
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    home: tempfile::TempDir,
    model: mock_model::MockServer,
    child: Child,
    client: reqwest::blocking::Client,
    base: String,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if std::thread::panicking() {
            if let Ok(log) = std::fs::read_to_string(self.home.path().join("core.stderr.log")) {
                eprintln!("Core stderr:\n{log}");
            }
        }
    }
}
fn wait(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < deadline, "workflow timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn step(decision: Value) -> Value {
    json!({"body":{"output_text":decision.to_string()}})
}
fn delegate(home: &Path) -> Value {
    json!({"kind":"delegate","project":home,"reason":"核实工作日资料","task":"仅核实假日和调休，返回来源和适用年份；不创建任务、不发送日报。"})
}
fn proposal() -> Value {
    json!({"kind":"tool","name":"propose_automation","args":{
        "title":"工作日报","goal":"根据已授权来源整理日报，交付到当前会话",
        "schedule":{"kind":"daily","hour":11,"minute":57,"second":0,"utc_offset_minutes":480},
        "calendar":{"weekdays":[1,2,3,4,5],"include_dates":[],"exclude_dates":[{"date":"2026-10-01","source":"fixture holiday source","reason":"fixture holiday"}]},
        "confirmation":{"before_seconds":600,"on_timeout":"run"},
        "time_window":{"kind":"previous_effective_day","hour":11,"minute":57,"second":0}
    }})
}
impl Fixture {
    fn new(steps: impl FnOnce(&Path) -> Vec<Value>, script: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let model = mock_model::MockServer::start(
            "127.0.0.1:0",
            serde_json::from_value(json!(steps(home.path()))).unwrap(),
        )
        .unwrap();
        std::fs::write(home.path().join("agent.sh"), script).unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            format!(
                r#"
model_provider = "fixture"
model = "fixture"
[model_providers.fixture]
base_url = "{}"
[model_providers.fixture.auth]
command = "printf"
args = ["fixture"]
[agents.codex]
command = "sh"
protocol = "cli"
args = ["{}"]
plugin_transport = "disabled"
"#,
                model.url,
                home.path().join("agent.sh").display()
            ),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let child = Command::new(env!("CARGO_BIN_EXE_omnistream"))
            .args(["serve", &addr.to_string()])
            .env("OMNISTREAM_HOME", home.path())
            .current_dir(home.path())
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                std::fs::File::create(home.path().join("core.stderr.log")).unwrap(),
            ))
            .spawn()
            .unwrap();
        let fixture = Self {
            home,
            model,
            child,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            base: format!("http://{addr}"),
        };
        wait(|| {
            fixture
                .client
                .get(format!("{}/health", fixture.base))
                .send()
                .is_ok()
        });
        fixture
    }
    fn post(&self, path: &str, body: Value) -> Value {
        self.client
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn chat(&self) {
        let response = self.client.post(format!("{}/chat/stream",self.base)).json(&json!({"conversation":"test","project":self.home.path(),"messages":[{"role":"user","content":"创建中国工作日11:57日报，提前10分钟确认，未确认仍执行。"}],"utc_offset_minutes":480})).send().unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().unwrap()
        );
    }
    fn detail(&self) -> Value {
        self.post("/conversation/detail", json!({"conversation":"test"}))
    }
    fn automations(&self) -> Value {
        self.client
            .get(format!("{}/automations", self.base))
            .send()
            .unwrap()
            .json()
            .unwrap()
    }
    fn wait_message(&self, needle: &str) {
        wait(|| {
            self.detail()["history"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["content"].as_str().is_some_and(|s| s.contains(needle)))
        });
    }
}
const AGENT: &str = "printf '%s' \"$1\" > prompt.txt\nprintf 'fixture holiday source: 2026-10-01 holiday; coverage 2026 only'\n";

#[test]
fn subtask_returns_to_manager_and_creates_only_a_pending_draft() {
    let f = Fixture::new(
        |home| {
            vec![
                step(json!({"kind":"tool","name":"list_automations","args":{}})),
                step(delegate(home)),
                step(proposal()),
                step(json!({"kind":"reply","message":"草案已保存，请在长期任务面板确认启用"})),
            ]
        },
        AGENT,
    );
    f.chat();
    f.wait_message("草案已保存");
    let items = f.automations();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["status"], "pending");
    assert_eq!(items[0]["conversation"], "test");
    assert_eq!(items[0]["confirmation"]["on_timeout"], "run");
    assert_eq!(items[0]["runs"], json!([]));
    let detail = f.detail();
    assert!(detail["calls"][0]["task"]["title"]
        .as_str()
        .unwrap()
        .starts_with("仅核实假日和调休"));
    let call = detail["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m.get("call_id").is_some())
        .unwrap();
    assert_eq!(call["reason"], "核实工作日资料");
    assert!(call["subtask"].as_str().unwrap().contains("不创建任务"));
    let prompt = std::fs::read_to_string(f.home.path().join("prompt.txt")).unwrap();
    assert!(prompt.contains("本次子任务：仅核实假日和调休"));
    assert!(prompt.contains("不是要求你完成全部流程"));
    let requests = f.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    let resumed = requests[2]["body"]["input"].as_str().unwrap();
    assert!(resumed.contains("fixture holiday source: 2026-10-01"));
    assert!(resumed.contains("工具调用 list_automations"));
    assert!(resumed.contains("不可信数据"));
}

#[test]
fn failed_subtask_is_reported_to_manager_without_creating_a_draft() {
    let f = Fixture::new(
        |home| {
            vec![
                step(delegate(home)),
                step(json!({"kind":"reply","message":"无法核实假日来源，未创建草案"})),
            ]
        },
        "echo fixture-source-failed >&2\nexit 1\n",
    );
    f.chat();
    f.wait_message("无法核实假日来源");
    assert_eq!(f.automations(), json!([]));
    let requests = f.model.requests.lock().unwrap();
    let resumed = requests[1]["body"]["input"].as_str().unwrap();
    assert!(resumed.contains("fixture-source-failed"));
    assert!(resumed.contains("\"status\":\"failed\""));
}

#[test]
fn plugin_write_ledger_survives_subtask_round_trip() {
    let write = json!({"kind":"tool","name":"plugin_invoke","args":{"plugin":"slack","action":"send_message","input":{"channel":"C123","text":"do not retry"}}});
    let f = Fixture::new(
        |home| vec![step(write.clone()), step(delegate(home)), step(write)],
        AGENT,
    );
    f.chat();
    f.wait_message("重复插件写入");
    let detail = f.detail();
    assert_eq!(
        detail["history"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["tool"] == "plugin_invoke")
            .count(),
        1
    );
    assert_eq!(f.automations(), json!([]));
}

#[test]
fn cancellation_while_manager_resumes_blocks_the_proposed_write() {
    let f = Fixture::new(
        |home| {
            let mut delayed = step(proposal());
            delayed["delay_ms"] = json!(400);
            vec![step(delegate(home)), delayed]
        },
        AGENT,
    );
    f.chat();
    wait(|| f.model.requests.lock().unwrap().len() == 2);
    let id = f.detail()["calls"][0]["task_id"].as_u64().unwrap();
    f.post(&format!("/agent-calls/{id}/cancel"), json!({}));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(f.automations(), json!([]));
    assert_eq!(f.model.requests.lock().unwrap().len(), 2);
}

#[test]
fn deleted_conversation_does_not_resume_or_get_recreated() {
    let f = Fixture::new(
        |home| {
            let mut delayed = step(proposal());
            delayed["delay_ms"] = json!(400);
            vec![step(delegate(home)), delayed]
        },
        AGENT,
    );
    f.chat();
    wait(|| f.model.requests.lock().unwrap().len() == 2);
    f.post("/conversation/delete", json!({"conversation":"test"}));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(f.detail()["history"], json!([]));
    assert_eq!(f.automations(), json!([]));
}

#[test]
fn repeated_subtasks_share_one_operation_budget() {
    let f = Fixture::new(
        |home| (0..17).map(|_| step(delegate(home))).collect(),
        AGENT,
    );
    f.chat();
    f.wait_message("超过最大轮数");
    assert_eq!(f.detail()["calls"].as_array().unwrap().len(), 16);
    assert_eq!(f.model.requests.lock().unwrap().len(), 17);
    assert_eq!(f.automations(), json!([]));
}

#[test]
fn missing_subtask_field_keeps_direct_agent_delivery() {
    let f = Fixture::new(
        |home| {
            let mut direct = delegate(home);
            direct.as_object_mut().unwrap().remove("task");
            vec![step(direct)]
        },
        AGENT,
    );
    f.chat();
    wait(|| f.detail()["calls"][0]["task"]["status"] == "unverified");
    assert_eq!(f.model.requests.lock().unwrap().len(), 1);
    assert_eq!(f.automations(), json!([]));
}

#[test]
fn new_user_message_supersedes_the_old_continuation() {
    let f = Fixture::new(
        |home| {
            let mut delayed = step(proposal());
            delayed["delay_ms"] = json!(400);
            vec![
                step(delegate(home)),
                delayed,
                step(json!({"kind":"reply","message":"已按新要求停止创建"})),
            ]
        },
        AGENT,
    );
    f.chat();
    wait(|| f.model.requests.lock().unwrap().len() == 2);
    let response = f
        .client
        .post(format!("{}/chat/stream", f.base))
        .json(&json!({"conversation":"test","messages":[{"role":"user","content":"停止创建"}]}))
        .send()
        .unwrap();
    assert!(response.status().is_success());
    f.wait_message("已按新要求停止创建");
    assert_eq!(f.automations(), json!([]));
    assert_eq!(
        f.detail()["history"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["tool"] == "propose_automation")
            .count(),
        0
    );
}

#[test]
fn manager_failure_after_subtask_is_visible_in_conversation() {
    let f = Fixture::new(
        |home| {
            vec![
                step(delegate(home)),
                json!({"status":500,"body":{"error":"fixture manager unavailable"}}),
            ]
        },
        AGENT,
    );
    f.chat();
    f.wait_message("子任务后续处理停止");
    assert_eq!(f.automations(), json!([]));
}
