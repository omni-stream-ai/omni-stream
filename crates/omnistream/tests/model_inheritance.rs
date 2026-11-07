use serde_json::{json, Value};
use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

struct Fixture {
    home: tempfile::TempDir,
    child: Child,
    client: reqwest::blocking::Client,
    base: String,
    stop: Arc<AtomicBool>,
    release_model: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Value>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn wait(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < deadline, "workflow timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}
impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let upstream = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", upstream.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let release_model = Arc::new(AtomicBool::new(true));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = stop.clone();
        let released = release_model.clone();
        let records = requests.clone();
        let project = home.path().to_string_lossy().into_owned();
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let Ok(Some(mut request)) = upstream.recv_timeout(Duration::from_millis(20)) else {
                    continue;
                };
                let released = released.clone();
                let records = records.clone();
                let stopped = stopped.clone();
                let project = project.clone();
                std::thread::spawn(move || {
                    let body = if request.url().ends_with("/models") {
                        json!({"data":[{"id":"model-a"},{"id":"model-b"}]})
                    } else {
                        let mut raw = String::new();
                        request.as_reader().read_to_string(&mut raw).unwrap();
                        records
                            .lock()
                            .unwrap()
                            .push(serde_json::from_str::<Value>(&raw).unwrap());
                        while !released.load(Ordering::Relaxed) && !stopped.load(Ordering::Relaxed)
                        {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        json!({"output_text":json!({"kind":"delegate","project":project,"reason":"inspect"}).to_string()})
                    };
                    let _ = request.respond(
                        tiny_http::Response::from_string(body.to_string()).with_header(
                            tiny_http::Header::from_bytes("Content-Type", "application/json")
                                .unwrap(),
                        ),
                    );
                });
            }
        });
        std::fs::write(home.path().join("agent.py"),r#"
import sys,os,json,tomllib,pathlib
root=pathlib.Path(__file__).parent
args=sys.argv[1:]
effective={}
for i,arg in enumerate(args[:-1]):
 if arg=='-c': effective.update(tomllib.loads(args[i+1]))
key=os.environ.get('OMNISTREAM_DELEGATED_API_KEY')
source='A' if key=='fixture-key-A' else 'B' if key=='fixture-key-B' else 'missing'
record={'effective':effective,'args':args,'credential_source':source}
with (root/'launches.jsonl').open('a') as out: out.write(json.dumps(record)+'\n')
thread='thread-'+str(len((root/'launches.jsonl').read_text().splitlines()))
for line in sys.stdin:
 m=json.loads(line)
 with (root/'rpc.jsonl').open('a') as out: out.write(json.dumps(m)+'\n')
 method=m.get('method')
 if method=='initialize': result={}
 elif method in ['thread/start','thread/resume','thread/fork']:
  if method=='thread/resume': thread=m['params']['threadId']
  result={'thread':{'id':thread}}
 elif method=='turn/start':
  print(json.dumps({'id':m['id'],'result':{'turn':{'id':'turn'}}}),flush=True)
  print(json.dumps({'method':'item/completed','params':{'threadId':thread,'turnId':'turn','item':{'type':'agentMessage','text':'fixture-finished'}}}),flush=True)
  print(json.dumps({'method':'turn/completed','params':{'threadId':thread,'turn':{'id':'turn','status':'completed'}}}),flush=True)
  continue
 else: continue
 print(json.dumps({'id':m['id'],'result':result}),flush=True)
"#).unwrap();
        std::fs::write(home.path().join("native.toml"),"model='native-model'\nmodel_provider='native-provider'\nmodel_reasoning_effort='xhigh'\n").unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            format!(
                r#"
model_provider='a'
model='model-a'
model_reasoning_effort='high'
[model_providers.a]
base_url='{origin}/a'
wire_api='responses'
[model_providers.a.auth]
command='printf'
args=['fixture-key-A']
[model_providers.b]
base_url='{origin}/b'
wire_api='responses'
[model_providers.b.auth]
command='printf'
args=['fixture-key-B']
[model_providers.incompatible]
base_url='{origin}/incompatible'
wire_api='chat_completions'
[model_providers.incompatible.auth]
command='printf'
args=['fixture-key-A']
[agents.codex]
command='python3'
protocol='app-server'
config_file='native.toml'
args=['{}','app-server','-c','model="argument-model"']
"#,
                home.path().join("agent.py").display()
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
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let f = Self {
            home,
            child,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            base: format!("http://{addr}"),
            stop,
            release_model,
            requests,
        };
        wait(|| f.client.get(format!("{}/health", f.base)).send().is_ok());
        f
    }
    fn post(&self, path: &str, input: Value) -> Value {
        self.client
            .post(format!("{}{path}", self.base))
            .json(&input)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn select(&self, provider: &str, model: &str, reasoning: Option<&str>) {
        self.post(
            "/settings/models/selection",
            json!({"provider":provider,"model":model,"reasoning":reasoning}),
        );
    }
    fn chat(&self, agent: Option<u64>) {
        let response=self.client.post(format!("{}/chat/stream",self.base)).json(&json!({"conversation":"inherit","project":self.home.path(),"agent_id":agent,"messages":[{"role":"user","content":"inspect"}]})).send().unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().unwrap()
        );
    }
    fn detail(&self) -> Value {
        self.post("/conversation/detail", json!({"conversation":"inherit"}))
    }
    fn wait_calls(&self, count: usize) {
        wait(|| {
            let detail = self.detail();
            let calls = detail["calls"].as_array().unwrap();
            calls.len() == count && calls.iter().all(|c| c["task"]["status"] == "unverified")
        });
    }
    fn records(&self, name: &str) -> Vec<Value> {
        std::fs::read_to_string(self.home.path().join(name))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

#[test]
fn delegation_pins_request_selection_overrides_native_config_and_isolates_resume() {
    let f = Fixture::new();
    f.release_model.store(false, Ordering::Relaxed);
    std::thread::scope(|scope| {
        let started = scope.spawn(|| f.chat(None));
        wait(|| f.requests.lock().unwrap().len() == 1);
        f.select("b", "model-b", Some("low"));
        f.release_model.store(true, Ordering::Relaxed);
        started.join().unwrap();
    });
    f.wait_calls(1);
    f.chat(None);
    f.wait_calls(2);
    f.chat(None);
    f.wait_calls(3);
    f.select("b", "model-b", None);
    f.chat(None);
    f.wait_calls(4);
    let launches = f.records("launches.jsonl");
    assert_eq!(launches.len(), 4);
    for (i, (model, effort, credential)) in [
        ("model-a", "high", "A"),
        ("model-b", "low", "B"),
        ("model-b", "low", "B"),
        ("model-b", "medium", "B"),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(launches[i]["effective"]["model"], *model);
        assert_eq!(launches[i]["effective"]["model_reasoning_effort"], *effort);
        assert_eq!(
            launches[i]["effective"]["model_provider"],
            "omnistream_inherited"
        );
        assert_eq!(launches[i]["credential_source"], *credential);
        assert!(!launches[i].to_string().contains("fixture-key-"));
    }
    let detail = f.detail();
    let calls = detail["calls"].as_array().unwrap();
    assert_ne!(calls[0]["scope"], calls[1]["scope"]);
    assert_eq!(calls[1]["scope"], calls[2]["scope"]);
    assert_ne!(calls[2]["scope"], calls[3]["scope"]);
    let rpc = f.records("rpc.jsonl");
    let threads: Vec<_> = rpc
        .iter()
        .filter(|r| matches!(r["method"].as_str(), Some("thread/start" | "thread/resume")))
        .collect();
    assert_eq!(threads[2]["method"], "thread/resume");
    assert_eq!(threads[2]["params"]["model"], "model-b");
    assert_eq!(
        threads[2]["params"]["modelProvider"],
        "omnistream_inherited"
    );
    assert!(!detail.to_string().contains("fixture-key-"));
    assert_eq!(
        std::fs::read_to_string(f.home.path().join("native.toml")).unwrap(),
        "model='native-model'\nmodel_provider='native-provider'\nmodel_reasoning_effort='xhigh'\n"
    );
}

#[test]
fn explicit_named_agent_inherits_selection_without_calling_manager() {
    let f = Fixture::new();
    f.select("b", "model-b", Some("high"));
    let agent=f.post("/agents",json!({"name":"researcher","description":"","instructions":"Read only","engine":"codex","plugins":[]}));
    f.chat(Some(agent["id"].as_u64().unwrap()));
    f.wait_calls(1);
    assert!(f.requests.lock().unwrap().is_empty());
    let launches = f.records("launches.jsonl");
    assert_eq!(launches[0]["effective"]["model"], "model-b");
    assert_eq!(launches[0]["credential_source"], "B");
}

#[test]
fn unsupported_selected_provider_fails_without_launching_native_model() {
    let f = Fixture::new();
    f.select("incompatible", "model-b", Some("low"));
    let agent=f.post("/agents",json!({"name":"researcher","description":"","instructions":"Read only","engine":"codex","plugins":[]}));
    f.chat(Some(agent["id"].as_u64().unwrap()));
    wait(|| f.detail()["calls"][0]["task"]["status"] == "failed");
    assert!(f.detail()["calls"][0]["task"]["last_output"]
        .as_str()
        .unwrap()
        .contains("仅支持 responses"));
    assert!(f.records("launches.jsonl").is_empty());
}
