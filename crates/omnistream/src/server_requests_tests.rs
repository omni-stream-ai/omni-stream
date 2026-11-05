#[cfg(test)]
mod tests {
    use crate::{
        adapter::{ApprovalDecision, ApprovalRequest},
        server_requests,
    };
    use serde_json::json;
    #[test]
    fn validates_user_answers_options_and_mcp_forms() {
        let request = ApprovalRequest {
            id: json!(1),
            method: "item/tool/requestUserInput".into(),
            params: json!({"questions":[{"id":"mode","question":"Mode","options":[{"label":"safe"},{"label":"fast"}]}]}),
        };
        let good = json!({"answers":{"mode":{"answers":["safe"]}}});
        assert!(server_requests::validate(&request, &good).is_ok());
        assert!(server_requests::validate(
            &request,
            &json!({"answers":{"mode":{"answers":["unsafe"]}}})
        )
        .is_err());
        assert!(server_requests::validate(&request, &json!({"answers":{}})).is_err());
        let mcp = ApprovalRequest {
            id: json!(2),
            method: "mcpServer/elicitation/request".into(),
            params: json!({"mode":"form","requestedSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}),
        };
        assert!(server_requests::validate(
            &mcp,
            &json!({"action":"accept","content":{"name":"x"}})
        )
        .is_ok());
        assert!(
            server_requests::validate(&mcp, &json!({"action":"accept","content":{"name":3}}))
                .is_err()
        );
        assert!(
            server_requests::validate(&mcp, &json!({"action":"decline","content":null})).is_ok()
        );
        assert_eq!(
            server_requests::decision(&mcp, ApprovalDecision::Decline).unwrap()["action"],
            "decline"
        );
    }
    #[test]
    fn permissions_are_subset_and_command_decisions_respect_available() {
        let request = ApprovalRequest {
            id: json!(1),
            method: "item/permissions/requestApproval".into(),
            params: json!({"permissions":{"network":{"enabled":true},"fileSystem":{"read":["/tmp"]}}}),
        };
        assert!(server_requests::validate(
            &request,
            &json!({"permissions":{"network":{"enabled":true}},"scope":"session"})
        )
        .is_ok());
        assert!(server_requests::validate(&request,&json!({"permissions":{"network":{"enabled":false},"fileSystem":{"read":["/root"]}},"scope":"turn"})).is_err());
        let command = ApprovalRequest {
            id: json!(1),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({"availableDecisions":["decline","cancel"]}),
        };
        assert!(server_requests::validate(&command, &json!({"decision":"accept"})).is_err());
        assert!(server_requests::validate(&command, &json!({"decision":"decline"})).is_ok());
    }
}
