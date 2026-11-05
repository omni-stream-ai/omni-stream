//! Real Core + real stdio relay across Agent protocols; no account or model credentials.
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    fs,
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "workflow timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Fixture {
    home: tempfile::TempDir,
    child: Child,
    client: Client,
    base: String,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<(String, String, Value)>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl Fixture {
    fn start(protocol: &str, native: bool) -> Self {
        Self::start_with_binary(
            protocol,
            native,
            std::path::Path::new(env!("CARGO_BIN_EXE_omnistream")),
        )
    }
    fn start_with_binary(protocol: &str, native: bool, binary: &std::path::Path) -> Self {
        let home = tempfile::tempdir().unwrap();
        let upstream = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", upstream.server_addr());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let recorded = requests.clone();
        let stopped = stop.clone();
        let project = home.path().to_string_lossy().into_owned();
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let mut request = match upstream.recv_timeout(Duration::from_millis(25)).unwrap() {
                    Some(r) => r,
                    None => continue,
                };
                let path = request.url().split('?').next().unwrap().to_string();
                let method = request.method().to_string();
                let mut data = String::new();
                request.as_reader().read_to_string(&mut data).unwrap();
                let input: Value = serde_json::from_str(&data).unwrap_or(json!({}));
                recorded
                    .lock()
                    .unwrap()
                    .push((method.clone(), path.clone(), input.clone()));
                let body = match path.as_str() {
                    "/responses" => {
                        json!({"output_text":json!({"kind":"delegate","project":project,"reason":"Use the configured plugin tools"}).to_string()})
                    }
                    "/slack/auth.test" => {
                        json!({"ok":true,"user":"fixture-user","user_id":"U123","team_id":"T123"})
                    }
                    "/github/user" => json!({"login":"fixture-user"}),
                    "/notion/users/me" => json!({"name":"fixture-user","id":"bot-1"}),
                    "/figma/me" => json!({"handle":"fixture-user"}),
                    "/slack/conversations.list" => {
                        json!({"ok":true,"channels":[{"id":"C123","name":"fixture-channel"}]})
                    }
                    "/slack/users.list" => {
                        json!({"ok":true,"members":[{"id":"U789","real_name":"JunJie Wang","deleted":false}],"response_metadata":{"next_cursor":"next-page"}})
                    }
                    "/slack/users.info" => {
                        json!({"ok":true,"user":{"id":"U789","real_name":"JunJie Wang"}})
                    }
                    "/github/user/repos" => json!([{"name":"fixture-repo","full_name":"org/repo"}]),
                    "/notion/search" => json!({"results":[{"id":"page-1","object":"page"}]}),
                    "/figma/files/File123" => {
                        json!({"name":"fixture-design","document":{"children":[]}})
                    }
                    "/github/repos/org/repo/issues" if method == "POST" => {
                        json!({"id":1,"number":1,"title":input["title"]})
                    }
                    "/slack/chat.postMessage" => {
                        json!({"ok":true,"channel":input["channel"],"ts":"1700000000.001","message":{"text":input["text"]}})
                    }
                    "/notion/pages" if method == "POST" => {
                        json!({"id":"page-created","object":"page"})
                    }
                    "/figma/files/File123/comments" if method == "POST" => {
                        json!({"id":"comment-created"})
                    }
                    _ => panic!("unexpected upstream: {method} {path}"),
                };
                let _ = request.respond(
                    tiny_http::Response::from_string(body.to_string()).with_header(
                        tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
                    ),
                );
            }
        });
        fs::write(home.path().join("agent-protocol"), protocol).unwrap();
        fs::write(
            home.path().join("agent.py"),
            include_str!("fixtures/plugin_agent.py"),
        )
        .unwrap();
        fs::write(
            home.path().join("mcp.py"),
            include_str!("fixtures/browser_mcp.py"),
        )
        .unwrap();
        let mut command = "python3".to_string();
        let mut args = vec![home.path().join("agent.py").to_string_lossy().into_owned()];
        if protocol == "app-server" {
            args.push("app-server".into());
        }
        if protocol == "cli" && native {
            fs::copy(home.path().join("agent.py"), home.path().join("claude")).unwrap();
            command = home.path().join("claude").to_string_lossy().into_owned();
            args.clear();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["mcp.py", "claude"] {
                let path = home.path().join(name);
                if path.exists() {
                    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
                }
            }
        }
        let mcp_args = if protocol == "jsonl" && native {
            "mcp_args=['--fixture-mcp','{mcp_config}']\n"
        } else {
            ""
        };
        fs::write(home.path().join("config.toml"),format!("model_provider='fixture'\nmodel='fixture'\n[model_providers.fixture]\nbase_url={origin:?}\nwire_api='responses'\n[model_providers.fixture.auth]\ncommand='printf'\nargs=['fixture']\n[agents.fixture]\ncommand={command:?}\nprotocol={protocol:?}\nargs={}\ntimeout_ms=30\n{mcp_args}",serde_json::to_string(&args).unwrap())).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let child = Command::new(binary)
            .args(["serve", &address.to_string()])
            .current_dir(home.path())
            .env("OMNISTREAM_HOME", home.path())
            .env("OMNISTREAM_PLUGIN_TEST_ORIGIN", origin)
            .env(
                "OMNISTREAM_BROWSER_MCP_TEST_COMMAND",
                home.path().join("mcp.py"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let fixture = Self {
            home,
            child,
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            base: format!("http://{address}"),
            stop,
            requests,
        };
        wait(|| {
            fixture
                .client
                .get(format!("{}/health", fixture.base))
                .send()
                .is_ok_and(|r| r.status().is_success())
        });
        fixture
    }
    fn post(&self, path: &str, input: &Value) -> Value {
        let response = self
            .client
            .post(format!("{}{path}", self.base))
            .json(input)
            .send()
            .unwrap();
        let status = response.status();
        let value: Value = response.json().unwrap();
        assert!(status.is_success(), "{path}: {status} {value}");
        value
    }
    fn connect(&self, id: &str, writes: bool) {
        self.post(&format!("/plugins/{id}/install"), &json!({}));
        if id == "chrome-devtools" {
            self.post(
                "/plugins/chrome-devtools/connection",
                &json!({"enabled":true}),
            );
        } else {
            self.configure(id, writes);
        }
    }
    fn configure(&self, id: &str, writes: bool) {
        self.post(
            &format!("/plugins/{id}/connection"),
            &json!({"enabled":true,"allow_writes":writes,"token":"fixture-private-token"}),
        );
    }
    fn start_chat(&self, cases: Value) {
        fs::write(self.home.path().join("tool-cases.json"), cases.to_string()).unwrap();
        let response = self.client.post(format!("{}/chat/stream",self.base)).json(&json!({"conversation":"plugins","messages":[{"role":"user","content":"Use my enabled plugins for the requested reads and writes"}]})).send().unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().unwrap()
        );
    }
    fn detail(&self) -> Value {
        self.post("/conversation/detail", &json!({"conversation":"plugins"}))
    }
    fn named_chat(&self, agent: u64, cases: Value) {
        self.named_chat_in(agent, "plugins", cases);
    }
    fn named_chat_in(&self, agent: u64, conversation: &str, cases: Value) {
        fs::write(self.home.path().join("tool-cases.json"), cases.to_string()).unwrap();
        let response=self.client.post(format!("{}/chat/stream",self.base)).json(&json!({"conversation":conversation,"agent_id":agent,"messages":[{"role":"user","content":"Run the explicitly selected assistant"}]})).send().unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().unwrap()
        );
    }
    fn automations(&self) -> Vec<Value> {
        self.client
            .get(format!("{}/automations", self.base))
            .send()
            .unwrap()
            .json()
            .unwrap()
    }
    fn finish(&self) -> Value {
        self.finish_in("plugins")
    }
    fn finish_in(&self, conversation: &str) -> Value {
        let mut detail = json!({});
        wait(|| {
            detail = self.post(
                "/conversation/detail",
                &json!({"conversation":conversation}),
            );
            detail["calls"].as_array().is_some_and(|calls| {
                !calls.is_empty()
                    && calls.iter().all(|call| {
                        !matches!(
                            call["task"]["status"].as_str(),
                            Some("running" | "queued" | "testing")
                        )
                    })
            })
        });
        assert_eq!(
            detail["calls"][0]["task"]["status"], "unverified",
            "{detail}"
        );
        detail
    }
    fn results(&self) -> Vec<Value> {
        serde_json::from_slice(&fs::read(self.home.path().join("tool-results.json")).unwrap())
            .unwrap()
    }
    fn run_automation(&self, run_id: u64, status: &str) -> Value {
        self.post("/automations/1/run", &json!({}));
        let mut item = Value::Null;
        wait(|| {
            let items: Vec<Value> = self
                .client
                .get(format!("{}/automations", self.base))
                .send()
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .unwrap();
            item = items[0].clone();
            item["runs"].as_array().is_some_and(|runs| {
                runs.last()
                    .is_some_and(|run| run["id"] == run_id && run["status"] != "running")
            })
        });
        assert_eq!(
            item["runs"].as_array().unwrap().last().unwrap()["status"],
            status,
            "{item}"
        );
        item
    }
    fn count_writes(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, path, _)| {
                method == "POST"
                    && [
                        "/github/repos/org/repo/issues",
                        "/slack/chat.postMessage",
                        "/notion/pages",
                        "/figma/files/File123/comments",
                    ]
                    .contains(&path.as_str())
            })
            .count()
    }
    fn approval(&self) -> Value {
        let mut approval = Value::Null;
        wait(|| {
            let detail = self.detail();
            approval = detail["calls"][0]["approvals"]
                .as_array()
                .and_then(|a| a.first())
                .cloned()
                .unwrap_or(Value::Null);
            approval.is_object()
        });
        approval
    }
}

#[test]
fn automation_agents_receive_context_and_only_update_their_authorized_dates() {
    for (protocol, native, allow_updates) in [
        ("cli", false, true),
        ("app-server", true, true),
        ("cli", false, false),
    ] {
        let fixture = Fixture::start(protocol, native);
        fixture.connect("slack", true);
        fixture.connect("chrome-devtools", false);
        let calendar = json!({"weekdays":[1,2,3,4,5],"include_dates":[{"date":"2099-11-07","source":"fixture","reason":"verified extra day"}],"exclude_dates":[]});
        fs::write(fixture.home.path().join("tool-cases.json"), json!([
            {"name":"automation_calendar"},
            {"name":"slack_list_channels"},
            {"name":"slack_send_message","arguments":{"channel":"C123","text":"blocked during generation"}},
            {"name":"automation_calendar","arguments":{"id":2,"calendar":calendar,"reason":"cannot target another task"}},
            {"name":"automation_calendar","arguments":{"calendar":{"weekdays":[7]},"reason":"cannot change base weekdays"}},
            {"name":"automation_calendar","arguments":{"calendar":calendar,"reason":"verified extra day"}},
            {"name":"automation_calendar"}
        ]).to_string()).unwrap();
        let created = fixture.post(
            "/automations",
            &json!({
                "title":"General progress","goal":"Collect progress in the supplied window",
                "schedule":{"kind":"interval","seconds":3600},
                "calendar":{"weekdays":[1,2,3,4,5]},"allow_calendar_updates":allow_updates,
                "time_window":{"kind":"lookback","seconds":7200},"conversation":"inbox"
            }),
        );
        let item = fixture.run_automation(1, "succeeded");
        let results = fixture.results();
        assert_eq!(results.len(), 7);
        for index in [0, 1, 6] {
            assert_ne!(results[index]["isError"], true, "{protocol}: {results:?}");
        }
        for index in [2, 3, 4] {
            assert_eq!(results[index]["isError"], true, "{protocol}: {results:?}");
        }
        assert_eq!(results[5]["isError"] == true, !allow_updates);
        assert_eq!(
            item["calendar"]["include_dates"].as_array().unwrap().len(),
            usize::from(allow_updates)
        );
        assert_eq!(
            item["calendar_audit"].as_array().unwrap().len(),
            1 + usize::from(allow_updates)
        );
        assert_eq!(item["calendar"]["weekdays"], json!([1, 2, 3, 4, 5]));
        assert_eq!(item["next_run_at"], created["next_run_at"]);
        let context = &item["runs"][0]["context"];
        assert_eq!(context["run_id"], 1);
        assert_eq!(
            context["time_window"]["end"].as_u64().unwrap()
                - context["time_window"]["start"].as_u64().unwrap(),
            7200
        );
        assert_eq!(context["calendar"]["include_dates"], json!([]));
        let workspace = fixture.home.path().join("workspace");
        assert_eq!(context["workspace"], workspace.to_str().unwrap());
        assert_eq!(
            fs::read_to_string(fixture.home.path().join("agent-cwd.txt")).unwrap(),
            workspace.to_str().unwrap()
        );
        let prompt = fs::read_to_string(fixture.home.path().join("agent-prompt.txt")).unwrap();
        assert!(prompt.contains("Collect progress in the supplied window"));
        let agent_context: Value = serde_json::from_str(
            prompt
                .lines()
                .find_map(|line| line.strip_prefix("运行上下文（时间为 Unix 秒，窗口左闭右开）："))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(agent_context["time_window"], context["time_window"]);
        assert_eq!(agent_context["workspace"], context["workspace"]);
        assert!(agent_context.get("delivery_grants").is_none());
        let tools: Value =
            serde_json::from_slice(&fs::read(fixture.home.path().join("tool-list.json")).unwrap())
                .unwrap();
        let names: Vec<_> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(names.contains(&"automation_calendar"));
        assert!(names.contains(&"slack_list_channels"));
        assert!(!names.contains(&"slack_send_message"));
        assert!(!names.iter().any(|name| name.starts_with("chrome_")));
        assert_eq!(fixture.count_writes(), 0);
    }
}

#[test]
fn automation_condition_false_is_silent_but_invalid_output_reports_failure() {
    let fixture = Fixture::start("cli", false);
    fixture.connect("slack", true);
    fs::write(fixture.home.path().join("tool-cases.json"), "[]").unwrap();
    let output_path = fixture.home.path().join("agent-output.txt");
    fs::write(
        &output_path,
        json!({"output_markdown":"Condition not yet met","condition_met":false}).to_string(),
    )
    .unwrap();
    fixture.post("/automations", &json!({
        "title":"Monitor a condition","goal":"Return the monitored condition as structured output",
        "schedule":{"kind":"interval","seconds":3600},"conversation":"inbox","silent_unmet":true,
        "delivery_targets":[{"type":"plugin_action","id":"report","plugin":"slack","action":"send_message","destination":{"channel":"C123"},"payload":{"text":"{{run.output_markdown}}"}}]
    }));
    let item = fixture.run_automation(1, "skipped");
    assert_eq!(item["runs"][0]["skip_reason"], "condition_unmet");
    assert_eq!(
        item["runs"][0]["generation_output"],
        "Condition not yet met"
    );
    assert_eq!(item["runs"][0]["deliveries"], json!([]));
    assert_eq!(
        fixture.post("/conversation/detail", &json!({"conversation":"inbox"}))["history"],
        json!([])
    );
    fs::write(
        &output_path,
        json!({"output_markdown":"Invalid condition","condition_met":"false"}).to_string(),
    )
    .unwrap();
    let item = fixture.run_automation(2, "failed");
    assert!(item["runs"][1]["skip_reason"].is_null());
    let detail = fixture.post("/conversation/detail", &json!({"conversation":"inbox"}));
    assert_eq!(detail["history"].as_array().unwrap().len(), 1);
    assert_eq!(detail["history"][0]["run_id"], 2);
    assert_eq!(fixture.count_writes(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn plugin_relays_survive_core_binary_replacement_and_session_resume() {
    for (protocol, native) in [("app-server", true), ("cli", false)] {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("omnistream");
        fs::copy(env!("CARGO_BIN_EXE_omnistream"), &binary).unwrap();
        let fixture = Fixture::start_with_binary(protocol, native, &binary);
        fixture.connect("slack", false);
        let hold = fixture.home.path().join("relay-hold");
        fs::write(&hold, "hold").unwrap();
        fixture.start_chat(json!([{"name":"slack_list_channels"}]));
        wait(|| fixture.home.path().join("relay-ready").exists());
        // Replace after the Agent has received its command, before spawning it.
        let replacement = directory.path().join("replacement");
        fs::write(&replacement, "not the running Core").unwrap();
        fs::rename(&replacement, &binary).unwrap();
        let running = std::path::PathBuf::from(format!("/proc/{}/exe", fixture.child.id()));
        assert!(fs::read_link(&running)
            .unwrap()
            .to_string_lossy()
            .ends_with(" (deleted)"));
        fs::remove_file(&hold).unwrap();
        fixture.finish();
        assert_ne!(fixture.results()[0]["isError"], true);

        // A new gateway must also work after replacement, with no disk binary left.
        fs::remove_file(&binary).unwrap();
        fs::remove_file(fixture.home.path().join("tool-completed")).unwrap();
        fixture.start_chat(json!([{"name":"slack_list_channels"}]));
        wait(|| {
            fixture.detail()["calls"]
                .as_array()
                .is_some_and(|calls| calls.len() == 2)
        });
        let detail = fixture.finish();
        assert!(
            detail["calls"]
                .as_array()
                .unwrap()
                .iter()
                .all(|call| call["task"]["status"] == "unverified"),
            "{detail}"
        );
        assert!(fixture.home.path().join("tool-completed").exists());
        assert_ne!(fixture.results()[0]["isError"], true);
        assert_eq!(
            fixture
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, path, _)| path == "/slack/conversations.list")
                .count(),
            2
        );
        if protocol == "app-server" {
            assert_eq!(
                fs::read_to_string(fixture.home.path().join("thread-methods")).unwrap(),
                "thread/start\nthread/resume\n"
            );
        }
        assert_eq!(fixture.count_writes(), 0);
    }
}

#[test]
fn agent_runs_and_plugin_gateways_ignore_legacy_timeouts_and_remain_cancellable() {
    for (protocol, native) in [("app-server", true), ("acp", true), ("cli", false)] {
        let fixture = Fixture::start(protocol, native);
        fixture.connect("slack", false);
        let hold = fixture.home.path().join("relay-hold");
        let ready = fixture.home.path().join("relay-ready");
        fs::write(&hold, "hold").unwrap();
        fixture.start_chat(json!([{"name":"slack_list_channels"}]));
        wait(|| ready.exists());
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            fixture.detail()["calls"][0]["task"]["status"],
            "running",
            "{protocol}"
        );
        fs::remove_file(&hold).unwrap();
        fixture.finish();
        assert_ne!(fixture.results()[0]["isError"], true);
        assert!(fixture.results()[0].to_string().contains("fixture-channel"));

        // An indefinitely waiting Agent must still stop on user cancellation.
        fs::remove_file(&ready).unwrap();
        fs::write(&hold, "hold").unwrap();
        fixture.start_chat(json!([{"name":"slack_list_channels"}]));
        wait(|| ready.exists());
        std::thread::sleep(Duration::from_millis(150));
        let detail = fixture.detail();
        let task = detail["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["task"]["status"] == "running")
            .expect("Agent must keep running beyond the old timeout")["task_id"]
            .as_u64()
            .unwrap();
        fixture.post(&format!("/agent-calls/{task}/cancel"), &json!({}));
        wait(|| {
            let detail = fixture.detail();
            let call = detail["calls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|call| call["task_id"] == task)
                .unwrap();
            call["task"]["status"] == "cancelled"
                && call["events"].as_array().unwrap().iter().any(|event| {
                    event["event"]["type"] == "failed"
                        && event["event"]["message"].as_str().is_some_and(|message| {
                            message.contains("取消") && !message.contains("超时")
                        })
                })
        });
        assert_eq!(fixture.count_writes(), 0);
    }
}

#[test]
fn occupied_thread_branches_persists_the_new_id_and_resumes_it_on_follow_up() {
    let fixture = Fixture::start("app-server", true);
    let id = fixture.post("/agents", &json!({"name":"Independent","instructions":"Use only authorized tools","engine":"fixture","plugins":[]}))["id"].as_u64().unwrap();
    fixture.named_chat(id, json!([{"name":"list_plugins"}]));
    fixture.finish();
    let database = rusqlite::Connection::open(fixture.home.path().join("omnistream.db")).unwrap();
    let original: (String, String) = database
        .query_row("SELECT scope,session_id FROM agent_sessions", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(original.1, "fixture-app-server");
    fs::write(fixture.home.path().join("thread-busy"), "external writer").unwrap();
    fixture.named_chat(id, json!([{"name":"agent_memory","arguments":{"operation":"upsert","key":"continued","content":"Completed in the independent thread"}}]));
    let detail = fixture.finish();
    let current: (String, String) = database
        .query_row("SELECT scope,session_id FROM agent_sessions", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(current.0, original.0);
    assert_eq!(current.1, "fixture-branch-app-server");
    assert!(detail["calls"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|call| call["events"].as_array().unwrap())
        .any(
            |event| event["event"]["method"] == "omnistream/thread/forked"
                && event["event"]["data"]["source_thread_id"] == original.1
        ));
    assert_ne!(fixture.results()[0]["isError"], true);
    fixture.named_chat(
        id,
        json!([{"name":"agent_memory","arguments":{"operation":"list"}}]),
    );
    fixture.finish();
    assert!(fixture.results()[0]
        .to_string()
        .contains("Completed in the independent thread"));
    assert_eq!(
        fs::read_to_string(fixture.home.path().join("thread-methods")).unwrap(),
        "thread/start\nthread/resume\nthread/fork\nthread/resume\n"
    );
    assert_eq!(fixture.count_writes(), 0);
}

#[test]
fn named_agents_remember_across_conversations_restart_and_scheduled_runs() {
    for (protocol, native) in [("app-server", true), ("cli", false)] {
        let mut fixture = Fixture::start(protocol, native);
        let definition = json!({"name":"Writer","instructions":"Work carefully","engine":"fixture","plugins":[]});
        let id = fixture.post("/agents", &definition)["id"].as_u64().unwrap();
        fixture.named_chat(id, json!([
            {"name":"agent_memory","arguments":{"operation":"list"}},
            {"name":"agent_memory","arguments":{"operation":"upsert","key":"report-style","content":"REPORT_STYLE_MARKER: concise with sources"}},
            {"name":"agent_memory","arguments":{"operation":"list","agent_id":999}},
            {"name":"agent_memory","arguments":{"operation":"list"}}
        ]));
        fixture.finish();
        let results = fixture.results();
        assert_ne!(results[1]["isError"], true);
        assert_eq!(results[2]["isError"], true);
        assert!(results[3].to_string().contains("REPORT_STYLE_MARKER"));
        assert!(
            fs::read_to_string(fixture.home.path().join("agent-prompt.txt"))
                .unwrap()
                .contains("主动调用 agent_memory")
        );
        assert_eq!(
            fixture
                .client
                .get(format!("{}/approvals", fixture.base))
                .send()
                .unwrap()
                .json::<Vec<Value>>()
                .unwrap()
                .len(),
            0,
            "Self memory must not require plugin-write approval"
        );

        let other = fixture.post(
            "/agents",
            &json!({"name":"Other","instructions":"Other","engine":"fixture","plugins":[]}),
        )["id"]
            .as_u64()
            .unwrap();
        fixture.named_chat(
            other,
            json!([{"name":"agent_memory","arguments":{"operation":"list"}}]),
        );
        fixture.finish();
        assert!(!fixture.results()[0]
            .to_string()
            .contains("REPORT_STYLE_MARKER"));
        assert!(
            !fs::read_to_string(fixture.home.path().join("agent-prompt.txt"))
                .unwrap()
                .contains("REPORT_STYLE_MARKER")
        );

        fixture.child.kill().unwrap();
        fixture.child.wait().unwrap();
        fixture.child = Command::new(env!("CARGO_BIN_EXE_omnistream"))
            .args(["serve", fixture.base.strip_prefix("http://").unwrap()])
            .current_dir(fixture.home.path())
            .env("OMNISTREAM_HOME", fixture.home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait(|| {
            fixture
                .client
                .get(format!("{}/health", fixture.base))
                .send()
                .is_ok_and(|r| r.status().is_success())
        });
        fixture.named_chat_in(
            id,
            "new-conversation",
            json!([{"name":"agent_memory","arguments":{"operation":"list"}}]),
        );
        fixture.finish_in("new-conversation");
        assert!(
            fs::read_to_string(fixture.home.path().join("agent-prompt.txt"))
                .unwrap()
                .contains("REPORT_STYLE_MARKER")
        );

        fs::write(fixture.home.path().join("tool-cases.json"), json!([
            {"name":"agent_memory","arguments":{"operation":"upsert","key":"follow-up","content":"FOLLOW_UP_MARKER: verify the next release"}},
            {"name":"agent_memory","arguments":{"operation":"delete","key":"report-style"}},
            {"name":"list_automations"}
        ]).to_string()).unwrap();
        let automation = fixture.post("/automations", &json!({"title":"Writer task","goal":"Use your memory","agent_id":id,"schedule":{"kind":"interval","seconds":3600}}))["id"].as_u64().unwrap();
        fixture.post(&format!("/automations/{automation}/run"), &json!({}));
        wait(|| fixture.automations()[0]["runs"][0]["status"] == "succeeded");
        assert_ne!(fixture.results()[0]["isError"], true);
        assert_ne!(fixture.results()[1]["isError"], true);
        assert_eq!(fixture.results()[2]["isError"], true);
        assert!(
            fs::read_to_string(fixture.home.path().join("agent-prompt.txt"))
                .unwrap()
                .contains("REPORT_STYLE_MARKER")
        );
        let notes: Value = fixture
            .client
            .get(format!("{}/agents/{id}/memory", fixture.base))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(notes.as_array().unwrap().len(), 1);
        assert_eq!(notes[0]["key"], "follow-up");
        fixture.named_chat_in(
            id,
            "after-task",
            json!([{"name":"agent_memory","arguments":{"operation":"list"}}]),
        );
        fixture.finish_in("after-task");
        let prompt = fs::read_to_string(fixture.home.path().join("agent-prompt.txt")).unwrap();
        assert!(prompt.contains("FOLLOW_UP_MARKER"));
        assert!(!prompt.contains("REPORT_STYLE_MARKER"));
        fixture.post(
            &format!("/agents/{id}/memory"),
            &json!({"operation":"delete","key":"follow-up"}),
        );
        assert_eq!(fixture.count_writes(), 0);
    }
}

#[test]
fn named_agents_route_explicitly_isolate_threads_and_bind_scheduled_tasks() {
    let fixture = Fixture::start("app-server", true);
    fixture.connect("slack", false);
    fixture.connect("github", false);
    let definition = json!({"name":"工作助手","description":"Work","instructions":"ROLE_MARKER: verify work sources","engine":"fixture","plugins":["slack"]});
    let profile = fixture.post("/agents", &definition);
    let id = profile["id"].as_u64().unwrap();
    fixture.named_chat(id,json!([
        {"name":"slack_list_channels"},
        {"name":"github_list_repositories"},
        {"name":"propose_automation","arguments":{"title":"Work digest","goal":"Summarize permitted sources","schedule":{"kind":"interval","seconds":3600}}}
    ]));
    fixture.finish();
    assert_ne!(fixture.results()[0]["isError"], true);
    assert_eq!(fixture.results()[1]["isError"], true);
    assert_ne!(fixture.results()[2]["isError"], true);
    let tools = fs::read_to_string(fixture.home.path().join("tool-list.json")).unwrap();
    assert!(tools.contains("propose_automation"));
    assert!(!tools.contains("github_list_repositories"));
    let task = fixture.automations().remove(0);
    assert_eq!(task["agent_id"], id);
    assert_eq!(task["conversation"], "plugins");
    assert_eq!(task["status"], "pending");
    assert!(task["runs"].as_array().unwrap().is_empty());

    let mut other = definition.clone();
    other["name"] = json!("Other");
    let second = fixture.post("/agents", &other)["id"].as_u64().unwrap();
    fixture.named_chat(second, json!([{ "name":"slack_list_channels" }]));
    fixture.finish();
    fixture.named_chat(id, json!([{ "name":"slack_list_channels" }]));
    fixture.finish();
    assert_eq!(
        fs::read_to_string(fixture.home.path().join("thread-methods")).unwrap(),
        "thread/start\nthread/start\nthread/resume\n"
    );
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, path, _)| path == "/responses")
            .count(),
        0,
        "Explicit @ routing must not ask the management model to select an Agent"
    );

    fs::write(
        fixture.home.path().join("tool-cases.json"),
        json!([{"name":"slack_list_channels"},{"name":"list_automations"}]).to_string(),
    )
    .unwrap();
    let automation = task["id"].as_u64().unwrap();
    fixture.post(&format!("/automations/{automation}/activate"), &json!({}));
    fixture.post(&format!("/automations/{automation}/run"), &json!({}));
    wait(|| {
        fixture.automations()[0]["runs"]
            .as_array()
            .is_some_and(|runs| runs.first().is_some_and(|run| run["status"] == "succeeded"))
    });
    assert_ne!(fixture.results()[0]["isError"], true);
    assert_eq!(
        fixture.results()[1]["isError"],
        true,
        "A scheduled run must not gain general task-management tools"
    );
    let prompts = fs::read_to_string(fixture.home.path().join("agent-prompts.jsonl")).unwrap();
    assert!(prompts.lines().all(|line| line.contains("ROLE_MARKER")));

    fixture
        .client
        .delete(format!("{}/agents/{id}", fixture.base))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    fixture.post(&format!("/automations/{automation}/run"), &json!({}));
    wait(|| {
        fixture.automations()[0]["runs"]
            .as_array()
            .is_some_and(|runs| runs.len() == 2 && runs[1]["status"] == "failed")
    });
    assert!(fixture.automations()[0]["runs"][1]["output"]
        .as_str()
        .unwrap()
        .contains("已删除"));
    assert_eq!(fixture.count_writes(), 0);
}

#[test]
fn changing_named_agent_plugin_scope_revokes_a_pending_write() {
    let fixture = Fixture::start("cli", false);
    fixture.connect("github", true);
    let mut definition = json!({"name":"Reviewer","instructions":"Use only explicitly authorized tools","engine":"fixture","plugins":["github"]});
    let id = fixture.post("/agents", &definition)["id"].as_u64().unwrap();
    fixture.named_chat(id,json!([{"name":"github_create_issue","arguments":{"owner":"org","repo":"repo","title":"Must be revoked"}}]));
    let approval = fixture.approval();
    definition["plugins"] = json!([]);
    fixture.post(&format!("/agents/{id}"), &definition);
    fixture.finish();
    assert_eq!(fixture.results()[0]["isError"], true);
    assert_eq!(fixture.count_writes(), 0);
    let response = fixture
        .client
        .post(format!(
            "{}/approvals/{}/approve",
            fixture.base, approval["id"]
        ))
        .json(&json!({}))
        .send()
        .unwrap();
    assert!(!response.status().is_success());
}

#[test]
fn all_agent_protocols_can_call_chrome_and_every_service_via_native_mcp_or_commands() {
    for (protocol, native) in [
        ("app-server", true),
        ("acp", true),
        ("cli", true),
        ("cli", false),
        ("jsonl", true),
        ("jsonl", false),
    ] {
        let fixture = Fixture::start(protocol, native);
        for id in ["slack", "github", "notion", "figma", "chrome-devtools"] {
            fixture.connect(id, false);
        }
        fixture.start_chat(json!([
            {"name":"list_plugins"},
            {"name":"slack_list_channels"},
            {"name":"github_list_repositories"},
            {"name":"notion_search","arguments":{}},
            {"name":"figma_get_file","arguments":{"file_key":"File123"}},
            {"name":"chrome_list_pages"},
            {"name":"chrome_evaluate_script","arguments":{"function":"()=>document.cookie"}},
            {"name":"github_create_issue","arguments":{"owner":"org","repo":"repo","title":"blocked"}}
        ]));
        fixture.finish();
        let results = fixture.results();
        assert_eq!(results.len(), 8, "{protocol} native={native}");
        for result in &results[..6] {
            assert_ne!(
                result["isError"], true,
                "{protocol} native={native}: {result}"
            );
        }
        assert_eq!(results[6]["isError"], true);
        assert_eq!(results[7]["isError"], true);
        assert_eq!(fixture.count_writes(), 0);
        let listed = fs::read_to_string(fixture.home.path().join("tool-list.json")).unwrap();
        assert!(!listed.contains("fixture-private-token"));
        assert!(!listed.contains("github_create_issue"));
        assert!(listed.contains("chrome_take_snapshot"));
    }
}

#[test]
fn service_writes_require_core_approval_and_execute_once_for_external_agents() {
    let fixture = Fixture::start("acp", true);
    for id in ["slack", "github", "notion", "figma"] {
        fixture.connect(id, true);
    }
    fixture.start_chat(json!([
        {"name":"github_create_issue","arguments":{"owner":"org","repo":"repo","title":"Requested issue"}},
        {"name":"slack_send_message","arguments":{"channel":"C123","text":"Requested message"}},
        {"name":"notion_create_page","arguments":{"parent_page_id":"page-1","title":"Requested page"}},
        {"name":"figma_post_comment","arguments":{"file_key":"File123","message":"Requested comment"}}
    ]));
    for count in 0..4 {
        let approval = fixture.approval();
        assert_eq!(approval["action"], "plugin.invoke");
        assert_eq!(approval["payload"]["conversation"], "plugins");
        assert_eq!(fixture.count_writes(), count);
        assert!(!approval.to_string().contains("fixture-private-token"));
        fixture.post(
            &format!("/approvals/{}/approve", approval["id"]),
            &json!({}),
        );
        wait(|| fixture.count_writes() == count + 1);
    }
    fixture.finish();
    assert_eq!(fixture.count_writes(), 4);
    assert!(fixture
        .results()
        .iter()
        .all(|result| result["isError"] != true));
}

#[test]
fn changing_an_account_or_permission_while_approval_is_pending_blocks_the_stale_write() {
    let fixture = Fixture::start("cli", false);
    fixture.connect("github", true);
    fixture.start_chat(json!([{"name":"github_create_issue","arguments":{"owner":"org","repo":"repo","title":"Stale request"}}]));
    let approval = fixture.approval();
    fixture.configure("github", false);
    fixture.post(
        &format!("/approvals/{}/approve", approval["id"]),
        &json!({}),
    );
    fixture.finish();
    assert_eq!(fixture.count_writes(), 0);
    assert_eq!(fixture.results()[0]["isError"], true);
}

#[test]
fn cancelling_an_agent_expires_pending_plugin_approval() {
    let fixture = Fixture::start("cli", false);
    fixture.connect("github", true);
    fixture.start_chat(json!([{"name":"github_create_issue","arguments":{"owner":"org","repo":"repo","title":"Cancelled request"}}]));
    let approval = fixture.approval();
    let task = fixture.detail()["calls"][0]["task_id"].as_u64().unwrap();
    fixture.post(&format!("/agent-calls/{task}/cancel"), &json!({}));
    wait(|| {
        fixture.detail()["calls"][0]["approvals"]
            .as_array()
            .is_some_and(|a| a.is_empty())
    });
    let response = fixture
        .client
        .post(format!(
            "{}/approvals/{}/approve",
            fixture.base, approval["id"]
        ))
        .json(&json!({}))
        .send()
        .unwrap();
    assert!(!response.status().is_success());
    assert_eq!(fixture.count_writes(), 0);
}

#[test]
fn slack_member_tools_work_through_mcp_and_command_gateway_without_write_access() {
    for native in [true, false] {
        let fixture = Fixture::start("cli", native);
        fixture.connect("slack", false);
        fixture.start_chat(json!([
            {"name":"slack_list_users","arguments":{"limit":100}},
            {"name":"slack_user_info","arguments":{"user":"U789"}}
        ]));
        fixture.finish();
        let results = fixture.results();
        assert_eq!(results.len(), 2);
        for result in &results {
            assert_ne!(result["isError"], true, "{result}");
            assert!(result.to_string().contains("JunJie Wang"), "{result}");
        }
        assert!(results[0].to_string().contains("next-page"));
        let listed = fs::read_to_string(fixture.home.path().join("tool-list.json")).unwrap();
        assert!(listed.contains("slack_list_users"));
        assert!(listed.contains("slack_user_info"));
        assert!(!listed.contains("slack_send_message"));
        assert!(!listed.contains("slack_open_conversation"));
        assert_eq!(fixture.count_writes(), 0);
    }
}

#[test]
fn confirmation_discussion_uses_task_agent_and_cannot_execute_or_approve() {
    for (protocol, native) in [("cli", false), ("app-server", true)] {
        let fixture = Fixture::start(protocol, native);
        fixture.connect("slack", true);
        let agent=fixture.post("/agents", &json!({"name":"Reviewer","instructions":"CONFIRMATION_ROLE_MARKER","engine":"fixture","plugins":["slack"]}))["id"].as_u64().unwrap();
        fixture.post("/automations",&json!({"title":"Review task","goal":"Do the business work only after approval","agent_id":agent,"schedule":{"kind":"interval","seconds":3600},"confirmation":{"before_seconds":60},"conversation":"review-inbox"}));
        fixture.post("/automations/1/run", &json!({}));
        wait(|| {
            !fixture.automations()[0]["confirmations"]
                .as_array()
                .unwrap()
                .is_empty()
        });
        assert!(fixture.automations()[0]["runs"]
            .as_array()
            .unwrap()
            .is_empty());
        // A task without a project uses Core engines, even if its default workspace has a local override.
        let local = fixture.home.path().join("workspace/.omnistream");
        fs::create_dir_all(&local).unwrap();
        fs::write(
            local.join("config.toml"),
            "[agents.fixture]\ncommand='false'\nprotocol='cli'\n",
        )
        .unwrap();
        let conversation = "automation-confirmation:1:1";
        // Starting a discussion increments revision before the scoped Agent gets tools.
        let cases = json!([
            {"name":"automation_confirmation"},
            {"name":"automation_confirmation","arguments":{"goal":"Adjusted business instructions","revision":2,"reason":"User asked for a change"}},
            {"name":"automation_confirmation","arguments":{"revision":3,"action":"approve"}},
            {"name":"automation_confirmation","arguments":{"id":2,"goal":"Wrong task","revision":3,"reason":"Forbidden"}},
            {"name":"slack_send_message","arguments":{"channel":"C123","text":"Must never be delivered"}},
            {"name":"run_automation","arguments":{"id":1}},
            {"name":"slack_list_channels"},
            {"name":"automation_confirmation"}
        ]);
        fs::write(
            fixture.home.path().join("tool-cases.json"),
            cases.to_string(),
        )
        .unwrap();
        fs::write(fixture.home.path().join("agent-hold"), "hold").unwrap();
        let response=fixture.client.post(format!("{}/chat/stream",fixture.base)).json(&json!({"conversation":conversation,"messages":[{"role":"user","content":"Adjust the instructions before I confirm"}]})).send().unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().unwrap()
        );
        wait(|| fixture.home.path().join("tool-completed").exists());
        let results = fixture.results();
        for index in [0, 1, 6, 7] {
            assert_ne!(results[index]["isError"], true, "{protocol}: {results:?}");
        }
        for index in [2, 3, 4, 5] {
            assert_eq!(results[index]["isError"], true, "{protocol}: {results:?}");
        }
        let attempt = fixture
            .client
            .post(format!(
                "{}/automations/1/confirmations/1/approve",
                fixture.base
            ))
            .json(&json!({"revision":3}))
            .send()
            .unwrap();
        assert!(
            !attempt.status().is_success(),
            "Must not approve during active discussion"
        );
        fs::remove_file(fixture.home.path().join("agent-hold")).unwrap();
        let detail = fixture.finish_in(conversation);
        assert_eq!(detail["calls"][0]["agent"], "Reviewer");
        assert_eq!(
            detail["calls"][0]["task"]["project"],
            fixture.home.path().join("workspace").to_str().unwrap()
        );
        let item = fixture.automations().remove(0);
        assert!(item["runs"].as_array().unwrap().is_empty());
        assert_eq!(
            item["confirmations"][0]["goal"],
            "Adjusted business instructions"
        );
        assert_eq!(item["confirmations"][0]["status"], "pending");
        assert_eq!(item["confirmations"][0]["discussion_active"], false);
        assert_eq!(item["goal"], "Do the business work only after approval");
        let prompt = fs::read_to_string(fixture.home.path().join("agent-prompt.txt")).unwrap();
        assert!(prompt.contains("CONFIRMATION_ROLE_MARKER"));
        assert!(prompt.contains("禁止执行委托"));
        assert_eq!(fixture.count_writes(), 0);
        let tools: Value =
            serde_json::from_slice(&fs::read(fixture.home.path().join("tool-list.json")).unwrap())
                .unwrap();
        let names: Vec<_> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"automation_confirmation"));
        assert!(!names.contains(&"run_automation"));
        assert!(!names.contains(&"slack_send_message"));
        // Revision must come from the latest saved proposal.
        let stale = fixture
            .client
            .post(format!(
                "{}/automations/1/confirmations/1/approve",
                fixture.base
            ))
            .json(&json!({"revision":2}))
            .send()
            .unwrap();
        assert!(!stale.status().is_success());
        fs::write(fixture.home.path().join("tool-cases.json"), "[]").unwrap();
        fixture.post(
            "/automations/1/confirmations/1/approve",
            &json!({"revision":3}),
        );
        wait(|| fixture.automations()[0]["runs"][0]["status"] == "succeeded");
        let run = &fixture.automations()[0]["runs"][0];
        assert_eq!(run["context"]["goal"], "Adjusted business instructions");
        assert_eq!(run["context"]["confirmation"]["revision"], 3);
        assert_eq!(fixture.count_writes(), 0);
        let expired=fixture.client.post(format!("{}/chat/stream",fixture.base)).json(&json!({"conversation":conversation,"messages":[{"role":"user","content":"Continue"}]})).send().unwrap();
        assert!(!expired.status().is_success());
        let fallback = fixture
            .client
            .post(format!("{}/chat", fixture.base))
            .json(&json!({"conversation":conversation,"message":"Continue"}))
            .send()
            .unwrap();
        assert!(!fallback.status().is_success());
    }
}
