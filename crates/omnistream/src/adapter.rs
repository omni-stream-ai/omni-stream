//! Core-facing contract. Wire protocols stay inside adapter implementations.
use crate::{config::AgentConfig, Config};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    Cli,
    Acp,
    Jsonl,
    AppServer,
    Mock,
}
impl Protocol {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("cli") {
            "cli" => Ok(Self::Cli),
            "acp" => Ok(Self::Acp),
            "jsonl" => Ok(Self::Jsonl),
            "app-server" => Ok(Self::AppServer),
            "mock" => Ok(Self::Mock),
            other => Err(format!("不支持的 Agent 协议：{other}")),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub resume: bool,
    pub approvals: bool,
    pub streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Failed { message: String },
    Session { id: String },
    Text { delta: String },
    Progress { method: String, data: Value },
    Completed { output: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Value,
    pub method: String,
    pub params: Value,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

pub struct Request<'a> {
    pub cwd: &'a Path,
    pub prompt: &'a str,
    pub session: Option<String>,
}

/// The host owns persistence and user decisions; adapters own wire translation.
#[allow(dead_code)]
pub trait Host {
    fn event(&mut self, event: Event) -> Result<(), String>;
    fn approve(&mut self, request: ApprovalRequest) -> Result<ApprovalDecision, String>;
    fn begin_request(&mut self, request: ApprovalRequest) -> Result<RequestResponse, String> {
        let decision = self.approve(request.clone())?;
        Ok(RequestResponse::Ready(crate::server_requests::decision(
            &request, decision,
        )?))
    }
    fn poll_response(&mut self, _ticket: u64) -> Result<Option<Value>, String> {
        Ok(None)
    }
    fn cancel_request(&mut self, _ticket: u64) -> Result<(), String> {
        Ok(())
    }
    fn close_requests(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn dynamic_tool(&mut self, _request: ApprovalRequest) -> Result<Value, String> {
        Err("Core 未注册该动态工具".into())
    }
}
#[allow(dead_code)]
pub enum RequestResponse {
    Ready(Value),
    Pending(u64),
}
pub trait Adapter {
    fn capabilities(&self) -> Capabilities;
    fn run(
        &self,
        request: Request<'_>,
        host: &mut dyn Host,
        cancel: Arc<AtomicBool>,
    ) -> Result<String, String>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub struct RecordingHost {
        pub events: Vec<Event>,
        pub approvals: Vec<ApprovalRequest>,
        pub decision: ApprovalDecision,
    }
    impl Default for RecordingHost {
        fn default() -> Self {
            Self {
                events: vec![],
                approvals: vec![],
                decision: ApprovalDecision::Accept,
            }
        }
    }
    impl Host for RecordingHost {
        fn event(&mut self, event: Event) -> Result<(), String> {
            self.events.push(event);
            Ok(())
        }
        fn approve(&mut self, request: ApprovalRequest) -> Result<ApprovalDecision, String> {
            self.approvals.push(request);
            Ok(self.decision)
        }
    }
    #[test]
    fn mock_contract_covers_resume_approval_failure_and_cancel() {
        let request = || Request {
            cwd: Path::new("."),
            prompt: "hello",
            session: Some("saved-session".into()),
        };
        let mut host = RecordingHost::default();
        let adapter = MockAdapter {
            approval: true,
            failure: false,
        };
        assert!(adapter
            .run(request(), &mut host, Arc::new(AtomicBool::new(false)))
            .is_ok());
        assert!(matches!(&host.events[0], Event::Session {id} if id == "saved-session"));
        assert_eq!(host.approvals.len(), 1);
        host.decision = ApprovalDecision::Decline;
        assert!(adapter
            .run(request(), &mut host, Arc::new(AtomicBool::new(false)))
            .unwrap_err()
            .contains("declined"));
        assert!(adapter
            .run(request(), &mut host, Arc::new(AtomicBool::new(true)))
            .unwrap_err()
            .contains("cancelled"));
        assert!(MockAdapter {
            approval: false,
            failure: true
        }
        .run(request(), &mut host, Arc::new(AtomicBool::new(false)))
        .is_err());
    }
}

pub struct Configured<'a> {
    pub agent: &'a AgentConfig,
    pub config: &'a Config,
}
impl Adapter for Configured<'_> {
    fn capabilities(&self) -> Capabilities {
        let native = matches!(self.agent.protocol.as_deref(), Some("app-server" | "mock"));
        Capabilities {
            resume: native,
            approvals: native,
            streaming: native,
        }
    }
    fn run(
        &self,
        request: Request<'_>,
        host: &mut dyn Host,
        cancel: Arc<AtomicBool>,
    ) -> Result<String, String> {
        let protocol = Protocol::parse(self.agent.protocol.as_deref())?;
        if request.session.is_some() && !self.capabilities().resume {
            return Err("适配器不支持会话恢复".into());
        }
        if protocol == Protocol::AppServer {
            return crate::app_server::run(self.agent, self.config, request, host, cancel);
        }
        if protocol == Protocol::Mock {
            return MockAdapter::default().run(request, host, cancel);
        }
        let mut last = String::new();
        let retries = if protocol == Protocol::Jsonl {
            0
        } else {
            self.agent.retries.unwrap_or(0)
        };
        for _ in 0..=retries {
            if cancel.load(Ordering::Relaxed) {
                return Err("Agent 已取消".into());
            }
            let result = match protocol {
                Protocol::Acp => crate::acp::run_with_mcp(
                    &self.agent.command,
                    &self.agent.args,
                    request.cwd,
                    request.prompt,
                    cancel.clone(),
                    &self.config.auto_approve,
                    self.config
                        .agent_mcp_servers
                        .as_array()
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                ),
                Protocol::Jsonl => crate::plugin::run_stdio_agent(
                    &self.agent.command,
                    &self.agent.args,
                    request.cwd,
                    request.prompt,
                ),
                Protocol::Cli => {
                    let mut args = self.agent.args.clone();
                    args.push(request.prompt.into());
                    crate::process::run_unbounded_with_cancel(
                        &self.agent.command,
                        &args,
                        request.cwd,
                        cancel.clone(),
                    )
                    .and_then(|r| {
                        if r.cancelled {
                            Err("Agent 已取消".into())
                        } else if r.success() {
                            Ok(
                                if self.config.plugin_setup || self.config.agent_stdout_only {
                                    r.stdout
                                } else {
                                    r.report()
                                },
                            )
                        } else {
                            Err(r.report())
                        }
                    })
                }
                _ => unreachable!(),
            };
            match result {
                Ok(output) => {
                    host.event(Event::Completed {
                        output: output.clone(),
                    })?;
                    return Ok(output);
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

#[derive(Default)]
pub struct MockAdapter {
    pub approval: bool,
    pub failure: bool,
}
impl Adapter for MockAdapter {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            resume: true,
            approvals: true,
            streaming: true,
        }
    }
    fn run(
        &self,
        request: Request<'_>,
        host: &mut dyn Host,
        cancel: Arc<AtomicBool>,
    ) -> Result<String, String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        host.event(Event::Session {
            id: request.session.unwrap_or_else(|| "mock-thread".into()),
        })?;
        if self.approval
            && !matches!(
                host.approve(ApprovalRequest {
                    id: serde_json::json!(1),
                    method: "command".into(),
                    params: serde_json::json!({})
                })?,
                ApprovalDecision::Accept
            )
        {
            return Err("declined".into());
        }
        if self.failure {
            return Err("mock failure".into());
        }
        let output = format!("mock: {}", request.prompt);
        host.event(Event::Text {
            delta: output.clone(),
        })?;
        host.event(Event::Completed {
            output: output.clone(),
        })?;
        Ok(output)
    }
}
