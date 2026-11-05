//! Optional public-network check. Uses a deliberately invalid constant token,
//! never account credentials, and only calls identity endpoints.
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Core(Child);
impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires outbound HTTPS; verifies rejection of an invalid token, not authorized reads or writes"]
fn plugin_official_identity_endpoints_reject_invalid_credentials_without_persisting_them() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let _core = Core(
        Command::new(env!("CARGO_BIN_EXE_omnistream"))
            .args(["serve", &address.to_string()])
            .current_dir(home.path())
            .env("OMNISTREAM_HOME", home.path())
            .env_remove("OMNISTREAM_PLUGIN_TEST_ORIGIN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{address}");
    let client = Client::builder()
        .timeout(Duration::from_secs(40))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if client
            .get(format!("{base}/health"))
            .send()
            .is_ok_and(|response| response.status().is_success())
        {
            break;
        }
        assert!(Instant::now() < deadline, "Core did not become ready");
        std::thread::sleep(Duration::from_millis(50));
    }
    const TOKEN: &str = "omnistream-invalid-test-token";
    std::thread::scope(|scope| {
        let mut checks = vec![];
        for id in ["slack", "github", "notion", "figma"] {
            let client = &client;
            let base = &base;
            let home = home.path();
            checks.push(scope.spawn(move || {
                assert!(client
                    .post(format!("{base}/plugins/{id}/install"))
                    .send()
                    .unwrap()
                    .status()
                    .is_success());
                let response = client
                    .post(format!("{base}/plugins/{id}/connection"))
                    .json(&json!({"token":TOKEN,"enabled":true,"allow_writes":false}))
                    .send()
                    .unwrap();
                assert_eq!(
                    response.status().as_u16(),
                    502,
                    "{id}: authentication failure must be reported"
                );
                let body: Value = response.json().unwrap();
                assert!(
                    !body.to_string().contains(TOKEN),
                    "{id}: token leaked in the Core response"
                );
                let error = body["error"].as_str().unwrap().to_ascii_lowercase();
                if id == "slack" {
                    assert!(error.contains("invalid_auth"), "{id}: {error}");
                } else {
                    assert!(error.contains("http 401"), "{id}: {error}");
                }
                assert!(
                    !home.join(format!("plugins/connections/{id}.json")).exists(),
                    "{id}: rejected credentials were persisted"
                );
                println!("{id}: official authentication rejection verified through Core");
            }));
        }
        for check in checks {
            check.join().unwrap();
        }
    });
    let listings: Vec<Value> = client
        .get(format!("{base}/plugins/marketplace"))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(listings.len(), 5);
    assert!(listings
        .iter()
        .filter(|listing| listing["manifest"]["runtime"] == "builtin")
        .all(
            |listing| listing["installed"] == true && listing["connection"]["configured"] == false
        ));
}
