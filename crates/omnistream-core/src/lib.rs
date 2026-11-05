use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub id: i64,
    pub task_id: Option<u64>,
    pub action: String,
    pub payload: Value,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDetails {
    pub method: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub reason: Option<String>,
    pub available_decisions: Vec<String>,
}

impl Approval {
    pub fn details(&self) -> ApprovalDetails {
        let request = self.payload.get("request").unwrap_or(&self.payload);
        let params = request.get("params").unwrap_or(request);
        let decisions = params
            .get("availableDecisions")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["accept".into(), "decline".into(), "cancel".into()]);
        ApprovalDetails {
            method: request
                .get("method")
                .and_then(Value::as_str)
                .map(String::from),
            command: params
                .get("command")
                .and_then(Value::as_str)
                .map(String::from),
            cwd: params.get("cwd").and_then(Value::as_str).map(String::from),
            reason: params
                .get("reason")
                .and_then(Value::as_str)
                .map(String::from),
            available_decisions: decisions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_human_approval_details() {
        let approval: Approval = serde_json::from_value(serde_json::json!({
            "id": 1, "task_id": 3, "action": "agent.request", "status": "pending",
            "payload": {"request": {"method": "item/commandExecution/requestApproval", "params": {
                "command": "pwd; rg --files", "cwd": "/repo", "reason": "inspect", "availableDecisions": ["accept", "cancel"]
            }}}
        })).unwrap();
        let details = approval.details();
        assert_eq!(details.command.as_deref(), Some("pwd; rg --files"));
        assert_eq!(details.cwd.as_deref(), Some("/repo"));
        assert_eq!(details.available_decisions, ["accept", "cancel"]);
    }
}
