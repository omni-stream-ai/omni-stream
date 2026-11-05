use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Output},
    time::Duration,
};

fn invoke(home: &std::path::Path, args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_omnistream"))
        .args(args)
        .env("OMNISTREAM_HOME", home)
        .current_dir(home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn delegation_returns_selected_agent_output_to_conversation() {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let path = home.path().to_path_buf();
    let mock = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let decision =
            serde_json::json!({"kind":"delegate","project":path,"reason":"read project files"});
        let body = serde_json::json!({"output_text":decision.to_string()}).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    });
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model_provider = "mock"
model = "test"
[model_providers.mock]
base_url = "http://{address}"
[model_providers.mock.auth]
command = "printf"
args = ["fixture-key"]
[agents.inspector]
command = "sh"
args = ["-c", "printf inspected-project"]
"#
        ),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_omnistream"))
        .env("OMNISTREAM_HOME", home.path())
        .current_dir(home.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "inspect this project\ninspector\nexit"
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    mock.join().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("inspector（cli）"), "{text}");
    assert!(text.contains("inspected-project"), "{text}");
    let db = rusqlite::Connection::open(home.path().join("omnistream.db")).unwrap();
    let history: String = db
        .query_row("SELECT data FROM conversations", [], |row| row.get(0))
        .unwrap();
    assert!(history.contains("inspected-project"));
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn natural_language_registers_project_without_switching() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let project_path = project.path().canonicalize().unwrap();
    let target = project_path.clone();
    let mock = std::thread::spawn(move || {
        for turn in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buffer[..n]);
                if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let decision = match turn {
                0 => {
                    serde_json::json!({"kind":"tool","name":"register_project","args":{"path":target,"name":"B"}})
                }
                1 => serde_json::json!({"kind":"tool","name":"list_projects","args":{}}),
                _ => {
                    let text = String::from_utf8_lossy(&request);
                    assert!(text.contains(target.to_str().unwrap()));
                    serde_json::json!({"kind":"reply","message":"registered B"})
                }
            };
            let body = serde_json::json!({"output_text":decision.to_string()}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        }
    });
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model_provider = "mock"
model = "test"
[model_providers.mock]
base_url = "http://{address}"
[model_providers.mock.auth]
command = "printf"
args = ["fixture-key"]
"#
        ),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_omnistream"))
        .env("OMNISTREAM_HOME", home.path())
        .current_dir(home.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "Register {} as B\nstatus\nexit",
        project_path.display()
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    mock.join().unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("registered B"), "{text}");
    assert!(
        text.contains(&format!("项目：{}，", home.path().display())),
        "{text}"
    );
    let db = rusqlite::Connection::open(home.path().join("omnistream.db")).unwrap();
    let path: String = db
        .query_row("SELECT path FROM projects WHERE name='B'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(path, project_path.to_string_lossy());
}

#[test]
fn chat_completions_provider_is_supported() {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mock = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                if request.len() < end + 4 + length {
                    continue;
                }
                assert!(headers.starts_with("post /v1/chat/completions "));
                let body: serde_json::Value = serde_json::from_slice(&request[end + 4..]).unwrap();
                assert_eq!(body["model"], "chat-model");
                assert_eq!(body["messages"][0]["role"], "system");
                assert_eq!(body["messages"][1]["role"], "user");
                break;
            }
        }
        let reply = serde_json::json!({"kind":"reply","message":"chat fixture response"});
        let body = serde_json::json!({"choices":[{"message":{"role":"assistant","content":reply.to_string()}}]}).to_string();
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model_provider = "mock"
model = "chat-model"
[model_providers.mock]
base_url = "http://{address}/v1"
wire_api = "chat_completions"
[model_providers.mock.auth]
command = "printf"
args = ["fixture-key"]
"#
        ),
    )
    .unwrap();
    let response = invoke(home.path(), &["hello chat fixture"]);
    assert!(String::from_utf8_lossy(&response.stdout).contains("chat fixture response"));
    mock.join().unwrap();
}
