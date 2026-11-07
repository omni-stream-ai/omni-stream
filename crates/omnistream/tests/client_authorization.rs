use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
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
fn start(home: &Path) -> (Core, Client, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let child = Command::new(env!("CARGO_BIN_EXE_omnistream"))
        .args(["serve", &address.to_string()])
        .env("OMNISTREAM_HOME", home)
        .env_remove("OMNISTREAM_DESKTOP_TOKEN")
        .current_dir(home)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let core = Core(child);
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let base = format!("http://{address}");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if client.get(format!("{base}/health")).send().is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "Core did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    (core, client, base)
}
fn token(byte: u8) -> String {
    URL_SAFE_NO_PAD.encode([byte; 32])
}
fn apply(client: &Client, base: &str, token: &str, name: &str) -> Value {
    client
        .post(format!("{base}/client-auth/requests"))
        .json(&json!({"name":name,"platform":"integration","token":token}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap()
}
fn cli(home: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_omnistream"))
        .arg("client-auth")
        .args(args)
        .env("OMNISTREAM_HOME", home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn remote_clients_bootstrap_approve_revoke_and_survive_restart() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "[access]\nrequire_auth=true\n[push]\nenabled=false\n",
    )
    .unwrap();
    let (core, client, base) = start(home.path());
    let health: Value = client
        .get(format!("{base}/health"))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        health,
        json!({"ok":true,"access":{"required":true,"authorized":false,"client_id":null}})
    );
    for (method, path) in [
        ("GET", "/projects"),
        ("GET", "/settings/models"),
        ("GET", "/events"),
        ("POST", "/chat/stream"),
        ("GET", "/push/status"),
        ("GET", "/client-auth/clients"),
    ] {
        let response = client
            .request(method.parse().unwrap(), format!("{base}{path}"))
            .json(&json!({}))
            .send()
            .unwrap();
        assert_eq!(response.status(), 401, "{path}");
    }
    let preflight = client
        .request(reqwest::Method::OPTIONS, format!("{base}/projects"))
        .header("Origin", "https://app.example")
        .send()
        .unwrap();
    assert_eq!(preflight.status(), 204);
    assert!(preflight.headers()["access-control-allow-headers"]
        .to_str()
        .unwrap()
        .contains("Authorization"));
    let a = token(1);
    let b = token(2);
    let first = apply(&client, &base, &a, "First");
    let first_id = first["id"].as_str().unwrap();
    let first_code = first["code"].as_str().unwrap();
    assert_eq!(first["status"], "pending");
    assert_eq!(
        client
            .get(format!("{base}/client-auth/requests"))
            .bearer_auth(&a)
            .send()
            .unwrap()
            .status(),
        401
    );
    let approval = cli(home.path(), &["approve", first_id, first_code]);
    assert!(!approval.contains(&a));
    assert!(!cli(home.path(), &["list"]).contains(&a));
    let second = apply(&client, &base, &b, "Second");
    let second_id = second["id"].as_str().unwrap();
    let second_path = format!("{base}/client-auth/requests/{second_id}");
    // Only a request's private key can poll it, including after approval.
    assert_eq!(
        client
            .get(&second_path)
            .bearer_auth(&a)
            .send()
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .get(format!("{base}/client-auth/requests/{first_id}"))
            .bearer_auth(&b)
            .send()
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .post(format!("{second_path}/approve"))
            .bearer_auth(&b)
            .json(&json!({"code":second["code"]}))
            .send()
            .unwrap()
            .status(),
        401
    );
    let pending: Value = client
        .get(format!("{base}/client-auth/requests"))
        .bearer_auth(&a)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert!(!pending.to_string().contains(&b));
    assert!(!pending.to_string().contains("token_hash"));
    assert_eq!(
        client
            .post(format!("{second_path}/approve"))
            .bearer_auth(&a)
            .json(&json!({"code":"wrong"}))
            .send()
            .unwrap()
            .status(),
        409
    );
    client
        .post(format!("{second_path}/approve"))
        .bearer_auth(&a)
        .json(&json!({"code":second["code"]}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        client
            .get(&second_path)
            .bearer_auth(&b)
            .send()
            .unwrap()
            .json::<Value>()
            .unwrap()["status"],
        "approved"
    );
    assert_eq!(
        client
            .get(format!("{base}/projects"))
            .bearer_auth(&b)
            .send()
            .unwrap()
            .status(),
        200
    );
    // Keep an authorized event stream open while another client revokes it.
    let mut events = TcpStream::connect(base.strip_prefix("http://").unwrap()).unwrap();
    events
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        events,
        "GET /events HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {b}\r\n\r\n",
        base.strip_prefix("http://").unwrap()
    )
    .unwrap();
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0u8];
        events.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
    }
    assert!(header.starts_with(b"HTTP/1.1 200"));
    client
        .delete(format!("{base}/client-auth/clients/{second_id}"))
        .bearer_auth(&a)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut remaining = Vec::new();
    events
        .read_to_end(&mut remaining)
        .expect("revoked SSE must close");
    assert_eq!(
        client
            .get(format!("{base}/projects"))
            .bearer_auth(&b)
            .send()
            .unwrap()
            .status(),
        401
    );
    drop(core);
    let (_core, client, base) = start(home.path());
    assert_eq!(
        client
            .get(format!("{base}/projects"))
            .bearer_auth(&a)
            .send()
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .get(format!("{base}/projects"))
            .bearer_auth(&b)
            .send()
            .unwrap()
            .status(),
        401
    );
    let db = std::fs::read(home.path().join("omnistream.db")).unwrap();
    assert!(!db.windows(a.len()).any(|window| window == a.as_bytes()));
    assert!(!db.windows(b.len()).any(|window| window == b.as_bytes()));
}
