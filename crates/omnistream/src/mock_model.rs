//! Scripted local HTTP model fixture, shared by integration tests and the CLI.
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Clone, Deserialize)]
pub struct Step {
    #[serde(default = "ok")]
    pub status: u16,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default)]
    pub disconnect: bool,
    #[serde(default)]
    pub body: Value,
    /// Optional fixture-only authentication assertion; headers are never recorded.
    #[serde(default)]
    pub expect_bearer: Option<String>,
    #[serde(default)]
    pub events: Vec<Value>,
}
fn ok() -> u16 {
    200
}
pub struct MockServer {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl MockServer {
    pub fn start(address: &str, steps: Vec<Step>) -> Result<Self, String> {
        let server = tiny_http::Server::http(address).map_err(|e| e.to_string())?;
        let url = format!("http://{}", server.server_addr());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let records = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut steps: VecDeque<_> = steps.into();
            while !stopping.load(Ordering::Relaxed) {
                let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(25)) else {
                    continue;
                };
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                records.lock().unwrap().push(serde_json::json!({"path":request.url(),"body":serde_json::from_str::<Value>(&body).unwrap_or(Value::String(body))}));
                let Some(step) = steps.pop_front() else {
                    let _ = request.respond(
                        tiny_http::Response::from_string("scenario exhausted")
                            .with_status_code(500),
                    );
                    continue;
                };
                if let Some(expected) = &step.expect_bearer {
                    let accepted = request.headers().iter().any(|header| {
                        header.field.equiv("Authorization")
                            && header.value.as_str() == format!("Bearer {expected}")
                    });
                    if !accepted {
                        let _ = request.respond(
                            tiny_http::Response::from_string("fixture authentication mismatch")
                                .with_status_code(401),
                        );
                        continue;
                    }
                }
                let deadline = std::time::Instant::now() + Duration::from_millis(step.delay_ms);
                while std::time::Instant::now() < deadline && !stopping.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                if step.disconnect {
                    drop(request.into_writer());
                    continue;
                }
                let (content_type, output) = if step.events.is_empty() {
                    ("application/json", step.body.to_string())
                } else {
                    (
                        "text/event-stream",
                        step.events
                            .iter()
                            .map(|event| {
                                format!(
                                    "event: {}\ndata: {event}\n\n",
                                    event["type"].as_str().unwrap_or("message")
                                )
                            })
                            .collect::<String>(),
                    )
                };
                let response = tiny_http::Response::from_string(output)
                    .with_status_code(step.status)
                    .with_header(
                        tiny_http::Header::from_bytes("Content-Type", content_type).unwrap(),
                    );
                let _ = request.respond(response);
            }
        });
        Ok(Self {
            url,
            requests,
            stop,
            worker: Some(worker),
        })
    }
}
impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts_json_sse_errors_delays_and_records_requests() {
        let steps: Vec<Step> = serde_json::from_value(serde_json::json!([
            {"body":{"choices":[{"message":{"content":"hello"}}]}},
            {"events":[{"type":"response.completed","response":{"output":[]}}]},
            {"status":429,"body":{"error":{"message":"rate limit"}}},
            {"delay_ms":100,"body":{}},
            {"disconnect":true}
        ]))
        .unwrap();
        let server = MockServer::start("127.0.0.1:0", steps).unwrap();
        let client = reqwest::blocking::Client::new();
        let first = client
            .post(format!("{}/v1/chat/completions", server.url))
            .json(&serde_json::json!({"model":"test"}))
            .send()
            .unwrap();
        assert!(first.text().unwrap().contains("hello"));
        let sse = client
            .post(format!("{}/v1/responses", server.url))
            .send()
            .unwrap();
        assert_eq!(sse.headers()["content-type"], "text/event-stream");
        assert!(sse.text().unwrap().contains("data:"));
        assert_eq!(client.get(&server.url).send().unwrap().status(), 429);
        assert!(client
            .get(&server.url)
            .timeout(Duration::from_millis(20))
            .send()
            .unwrap_err()
            .is_timeout());
        assert!(client
            .get(&server.url)
            .timeout(Duration::from_secs(1))
            .send()
            .is_err());
        let records = server.requests.lock().unwrap();
        assert_eq!(records.len(), 5);
        assert_eq!(records[0]["body"]["model"], "test");
    }
}
