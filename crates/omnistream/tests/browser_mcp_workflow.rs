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
            .env("OMNISTREAM_BROWSER_MCP_TEST_COMMAND", home.join("mcp.py"))
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
    fn post(&self, path: &str, value: &Value) -> reqwest::blocking::Response {
        self.client
            .post(format!("{}{path}", self.base))
            .json(value)
            .send()
            .unwrap()
    }
    fn install(&self, id: &str) {
        assert!(self
            .post(&format!("/plugins/{id}/install"), &json!({}))
            .status()
            .is_success());
    }
    fn market(&self) -> Value {
        self.client
            .get(format!("{}/plugins/marketplace", self.base))
            .send()
            .unwrap()
            .json()
            .unwrap()
    }
    fn finish(&self, conversation: &str, count: usize) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let detail: Value = self
                .post(
                    "/conversation/detail",
                    &json!({"conversation":conversation}),
                )
                .json()
                .unwrap();
            if detail["calls"].as_array().is_some_and(|calls| {
                calls.len() == count
                    && calls
                        .iter()
                        .all(|call| call["task"]["status"] == "unverified")
            }) {
                return detail;
            }
            assert!(Instant::now() < deadline, "{detail}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
fn fixture(home: &Path) {
    std::fs::write(home.join("mcp.py"), include_str!("fixtures/browser_mcp.py")).unwrap();
    std::fs::write(
        home.join("agent.py"),
        include_str!("fixtures/browser_agent.py"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(home.join("mcp.py"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    std::fs::write(
        home.join("agent-config.toml"),
        "model='fixture-model'\n[mcp_servers.unrelated]\ncommand='unrelated-fixture'\n",
    )
    .unwrap();
    std::fs::write(home.join("config.toml"), format!("[agents.z-cli]\ncommand='unused-cli'\n[agents.codex]\ncommand='python3'\nprotocol='app-server'\nconfig_file={:?}\nargs=[{:?},'app-server']\n",home.join("agent-config.toml").to_str().unwrap(),home.join("agent.py").to_str().unwrap())).unwrap();
}
fn setup_request() -> Value {
    json!({"request_id":"00000000-0000-0000-0000-000000000001","options":{"auth_mode":"slack_user","listen":true,"listen_scope":"related","allow_writes":false,"enabled":true,"locale":"zh-CN"}})
}

#[test]
fn browser_marketplace_configures_the_actual_agent_and_disable_or_uninstall_removes_the_tool() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    let core = Core::start(home.path());
    assert_eq!(core.market().as_array().unwrap().len(), 5);
    assert_eq!(
        core.post(
            "/plugins/chrome-devtools/connection",
            &json!({"enabled":true})
        )
        .status()
        .as_u16(),
        409
    );
    core.install("chrome-devtools");
    core.install("slack");
    assert_eq!(
        core.post("/plugins/chrome-devtools/test", &json!({}))
            .status()
            .as_u16(),
        409
    );
    assert_eq!(
        core.post(
            "/plugins/chrome-devtools/connection",
            &json!({"enabled":true,"token":"private-token"})
        )
        .status()
        .as_u16(),
        400
    );
    assert!(core
        .post(
            "/plugins/chrome-devtools/connection",
            &json!({"enabled":true})
        )
        .status()
        .is_success());
    let verified: Value = core
        .post("/plugins/chrome-devtools/test", &json!({}))
        .json()
        .unwrap();
    assert_eq!(verified["browser"]["status"], "ready");
    assert!(!verified.to_string().contains("private-page-fixture"));
    let started: Value = core
        .post("/plugins/slack/setup", &setup_request())
        .json()
        .unwrap();
    let conversation = started["conversation"].as_str().unwrap();
    let detail = core.finish(conversation, 1);
    assert_eq!(detail["calls"][0]["agent"], "codex");
    assert_eq!(
        detail["calls"][0]["task"]["last_output"],
        "Browser MCP enabled"
    );
    let enabled_scope = detail["calls"][0]["scope"].clone();
    let args: Vec<Value> = std::fs::read_to_string(home.path().join("agent-args.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(args[0]
        .to_string()
        .contains("mcp_servers.omnistream_plugins="));
    assert!(args[0].to_string().contains("unrelated-fixture"));
    assert!(args[0].to_string().contains("fixture-model"));
    assert!(
        std::fs::read_to_string(home.path().join("agent-turns.jsonl"))
            .unwrap()
            .contains("list_pages")
    );
    let original = std::fs::read_to_string(home.path().join("agent-config.toml")).unwrap();
    assert!(!original.contains("omnistream_chrome_devtools"));
    let send = |message: &str| json!({"conversation":conversation,"messages":[{"role":"user","content":message}]});
    assert!(core
        .post(
            "/plugins/chrome-devtools/connection",
            &json!({"enabled":false})
        )
        .status()
        .is_success());
    assert!(core
        .post("/chat/stream", &send("继续检查配置"))
        .status()
        .is_success());
    let detail = core.finish(conversation, 2);
    assert_eq!(
        detail["calls"][1]["task"]["last_output"],
        "Browser MCP disabled"
    );
    assert_ne!(detail["calls"][1]["scope"], enabled_scope);
    drop(core);
    let core = Core::start(home.path());
    let chrome = core
        .market()
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["manifest"]["id"] == "chrome-devtools")
        .unwrap()
        .clone();
    assert_eq!(chrome["browser"]["enabled"], false);
    assert!(core
        .post(
            "/plugins/chrome-devtools/connection",
            &json!({"enabled":true})
        )
        .status()
        .is_success());
    assert!(core
        .client
        .delete(format!("{}/plugins/chrome-devtools", core.base))
        .send()
        .unwrap()
        .status()
        .is_success());
    assert!(!home
        .path()
        .join("plugins/mcp/chrome-devtools.json")
        .exists());
    assert!(core
        .post("/chat/stream", &send("卸载后继续"))
        .status()
        .is_success());
    assert_eq!(
        core.finish(conversation, 3)["calls"][2]["task"]["last_output"],
        "Browser MCP disabled"
    );
}

#[test]
fn browser_probes_report_failures_and_cannot_restore_a_disabled_tool() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    let core = Core::start(home.path());
    core.install("chrome-devtools");
    core.install("slack");
    core.post(
        "/plugins/chrome-devtools/connection",
        &json!({"enabled":true}),
    );
    std::fs::write(home.path().join("browser-mode"), "disconnected").unwrap();
    let response = core.post("/plugins/chrome-devtools/test", &json!({}));
    assert_eq!(response.status().as_u16(), 502);
    let error = response.text().unwrap();
    assert!(error.contains("chrome://inspect"));
    assert!(!error.contains("private-page"));
    std::fs::write(home.path().join("browser-mode"), "missing").unwrap();
    assert_eq!(
        core.post("/plugins/slack/setup", &setup_request())
            .status()
            .as_u16(),
        502
    );
    let history: Value = core
        .post(
            "/conversation/detail",
            &json!({"conversation":"plugin-setup:slack:00000000-0000-0000-0000-000000000001"}),
        )
        .json()
        .unwrap();
    assert_eq!(history["history"], json!([]));
    std::fs::write(home.path().join("browser-mode"), "ok").unwrap();
    std::fs::remove_file(home.path().join("probe-started")).unwrap();
    std::fs::write(home.path().join("probe-delay"), "wait").unwrap();
    let client = core.client.clone();
    let url = format!("{}/plugins/chrome-devtools/test", core.base);
    let probing = std::thread::spawn(move || client.post(url).json(&json!({})).send().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !home.path().join("probe-started").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let start = Instant::now();
    assert!(core
        .post(
            "/plugins/chrome-devtools/connection",
            &json!({"enabled":false})
        )
        .status()
        .is_success());
    assert!(start.elapsed() < Duration::from_secs(1));
    std::fs::remove_file(home.path().join("probe-delay")).unwrap();
    assert_eq!(probing.join().unwrap().status().as_u16(), 409);
    let market = core.market();
    let chrome = market
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["manifest"]["id"] == "chrome-devtools")
        .unwrap();
    assert_eq!(chrome["browser"]["enabled"], false);
    assert!(chrome["browser"]["verified_at"].is_null());

    // Disabling during the setup preflight must also prevent a stale Agent launch.
    core.post(
        "/plugins/chrome-devtools/connection",
        &json!({"enabled":true}),
    );
    std::fs::remove_file(home.path().join("startup-started")).unwrap();
    std::fs::write(home.path().join("startup-delay"), "wait").unwrap();
    let client = core.client.clone();
    let url = format!("{}/plugins/slack/setup", core.base);
    let starting =
        std::thread::spawn(move || client.post(url).json(&setup_request()).send().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !home.path().join("startup-started").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    core.post(
        "/plugins/chrome-devtools/connection",
        &json!({"enabled":false}),
    );
    std::fs::remove_file(home.path().join("startup-delay")).unwrap();
    assert_eq!(starting.join().unwrap().status().as_u16(), 409);
    let history: Value = core
        .post(
            "/conversation/detail",
            &json!({"conversation":"plugin-setup:slack:00000000-0000-0000-0000-000000000001"}),
        )
        .json()
        .unwrap();
    assert_eq!(history["history"], json!([]));
}

#[test]
#[ignore = "requires Chrome and the pinned npm package; uses an isolated browser with no accounts or model"]
fn browser_real_chrome_and_official_mcp_connect_through_core() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    let profile = home.path().join("chrome-profile");
    let binary =
        std::env::var("PLAYWRIGHT_CHROMIUM_EXECUTABLE").unwrap_or_else(|_| "google-chrome".into());
    let mut command = Command::new(binary);
    command
        .args([
            "--headless",
            "--no-sandbox",
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    struct Browser(Child);
    impl Drop for Browser {
        fn drop(&mut self) {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _browser = Browser(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    let port = loop {
        if let Ok(data) = std::fs::read_to_string(profile.join("DevToolsActivePort")) {
            if let Some(port) = data
                .lines()
                .next()
                .and_then(|line| line.parse::<u16>().ok())
            {
                break port;
            }
        }
        assert!(Instant::now() < deadline, "Chrome did not start");
        std::thread::sleep(Duration::from_millis(25));
    };
    std::fs::write(home.path().join("mcp.py"),format!("#!/usr/bin/env sh\nexec npx --yes chrome-devtools-mcp@1.12.0 --browserUrl http://127.0.0.1:{port} --no-usage-statistics --no-performance-crux --no-javascript-evaluation --categoryNetwork=false --categoryPerformance=false --categoryMemory=false --categoryEmulation=false\n")).unwrap();
    let core = Core::start(home.path());
    core.install("chrome-devtools");
    assert!(core
        .post(
            "/plugins/chrome-devtools/connection",
            &json!({"enabled":true})
        )
        .status()
        .is_success());
    let response = core.post("/plugins/chrome-devtools/test", &json!({}));
    assert!(
        response.status().is_success(),
        "{}",
        response.text().unwrap()
    );
    let chrome = core
        .market()
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["manifest"]["id"] == "chrome-devtools")
        .unwrap()
        .clone();
    assert_eq!(chrome["browser"]["status"], "ready");
    assert!(!chrome.to_string().contains("about:blank"));
}
