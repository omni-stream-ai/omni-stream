//! App Server request semantics and user-response validation.
use crate::adapter::{ApprovalDecision, ApprovalRequest};
use serde_json::{json, Value};

pub fn interactive(method: &str) -> bool {
    matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
            | "item/tool/requestUserInput"
            | "mcpServer/elicitation/request"
            | "applyPatchApproval"
            | "execCommandApproval"
    )
}

pub fn decision(request: &ApprovalRequest, decision: ApprovalDecision) -> Result<Value, String> {
    let accepted = matches!(
        decision,
        ApprovalDecision::Accept | ApprovalDecision::AcceptForSession
    );
    Ok(match request.method.as_str() {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({"decision":match decision { ApprovalDecision::Accept => "accept", ApprovalDecision::AcceptForSession => "acceptForSession", ApprovalDecision::Decline => "decline", ApprovalDecision::Cancel => "cancel" }})
        }
        "applyPatchApproval" | "execCommandApproval" => {
            json!({"decision":match decision { ApprovalDecision::Accept | ApprovalDecision::AcceptForSession => "approved", ApprovalDecision::Decline => "denied", ApprovalDecision::Cancel => "abort" }})
        }
        "item/permissions/requestApproval" => {
            json!({"permissions":if accepted { request.params["permissions"].clone() } else { json!({}) },"scope":"turn"})
        }
        "item/tool/requestUserInput" if !accepted => json!({"answers":{}}),
        "mcpServer/elicitation/request" if !accepted || request.params["mode"] == "url" => {
            json!({"action":match decision { ApprovalDecision::Accept | ApprovalDecision::AcceptForSession => "accept", ApprovalDecision::Decline => "decline", ApprovalDecision::Cancel => "cancel" },"content":null})
        }
        _ => return Err("此请求需要具体回答，请使用 approval respond <id> <JSON>".into()),
    })
}

fn subset(granted: &Value, requested: &Value) -> bool {
    match granted {
        Value::Object(fields) => fields
            .iter()
            .all(|(k, v)| requested.get(k).is_some_and(|r| subset(v, r))),
        Value::Array(items) => requested
            .as_array()
            .is_some_and(|r| items.iter().all(|v| r.contains(v))),
        Value::Null => true,
        Value::Bool(false) => requested.is_boolean(),
        _ => granted == requested,
    }
}

pub fn validate(request: &ApprovalRequest, response: &Value) -> Result<(), String> {
    if !response.is_object() {
        return Err("响应必须是 JSON 对象".into());
    }
    match request.method.as_str() {
        "item/tool/requestUserInput" => {
            let answers = response["answers"].as_object().ok_or("需要 answers 对象")?;
            let questions = request.params["questions"]
                .as_array()
                .ok_or("请求缺少 questions")?;
            if answers.len() != questions.len() {
                return Err("需要回答全部问题，且不能包含未知问题 ID".into());
            }
            for q in questions {
                let id = q["id"].as_str().ok_or("问题缺少 ID")?;
                let values = answers
                    .get(id)
                    .and_then(|v| v["answers"].as_array())
                    .ok_or("每个问题需要 answers 字符串数组")?;
                if values.is_empty()
                    || values
                        .iter()
                        .any(|v| v.as_str().is_none_or(|s| s.trim().is_empty()))
                {
                    return Err("回答不能为空".into());
                }
                if let Some(options) = q["options"].as_array().filter(|v| !v.is_empty()) {
                    if q["isOther"] != true
                        && values
                            .iter()
                            .any(|v| !options.iter().any(|o| o["label"] == *v))
                    {
                        return Err("回答必须使用提供的选项标签".into());
                    }
                }
            }
        }
        "mcpServer/elicitation/request" => match response["action"].as_str() {
            Some("decline" | "cancel") => {
                if !response["content"].is_null() {
                    return Err("拒绝或取消时 content 必须为空".into());
                }
            }
            Some("accept") if request.params["mode"] == "url" => {
                if !response["content"].is_null() {
                    return Err("URL 确认不接受表单内容".into());
                }
            }
            Some("accept") => {
                let schema = &request.params["requestedSchema"];
                let validator = jsonschema::validator_for(schema)
                    .map_err(|e| format!("表单 schema 无效：{e}"))?;
                if !validator.is_valid(&response["content"]) {
                    return Err("content 不符合表单 schema".into());
                }
            }
            _ => return Err("action 必须为 accept/decline/cancel".into()),
        },
        "item/permissions/requestApproval" => {
            if !response["permissions"].is_object()
                || !subset(&response["permissions"], &request.params["permissions"])
            {
                return Err("只能授予请求中的权限子集".into());
            }
            if !response["scope"].is_null()
                && !matches!(response["scope"].as_str(), Some("turn" | "session"))
            {
                return Err("scope 必须为 turn/session".into());
            }
            if response
                .as_object()
                .unwrap()
                .keys()
                .any(|k| !matches!(k.as_str(), "permissions" | "scope"))
            {
                return Err("未知权限响应字段".into());
            }
        }
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            if !matches!(
                response["decision"].as_str(),
                Some("accept" | "acceptForSession" | "decline" | "cancel")
            ) {
                return Err("无效审批决定".into());
            }
            if let Some(available) = request.params["availableDecisions"].as_array() {
                if !available.contains(&response["decision"]) {
                    return Err("该审批决定不在允许列表中".into());
                }
            }
        }
        "applyPatchApproval" | "execCommandApproval" => {
            if !matches!(
                response["decision"].as_str(),
                Some("approved" | "approved_for_session" | "denied" | "abort")
            ) {
                return Err("无效旧版审批决定".into());
            }
        }
        _ => return Err("不支持此请求的人工响应".into()),
    }
    Ok(())
}
