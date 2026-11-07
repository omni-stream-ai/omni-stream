//! Durable user approval for one occurrence, separate from task activation.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmationConfig {
    pub before_seconds: u64,
    #[serde(default = "default_timeout")]
    pub on_timeout: String,
}
fn default_timeout() -> String {
    "skip".into()
}
impl ConfirmationConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=604800).contains(&self.before_seconds)
            || !["skip", "wait", "run"].contains(&self.on_timeout.as_str())
        {
            return Err("提前确认时间须为 1 秒至 7 天，未确认策略须为 skip、wait 或 run".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Confirmation {
    pub id: u64,
    pub scheduled_at: u64,
    pub requested_at: u64,
    pub request_at: u64,
    pub deadline: Option<u64>,
    pub trigger: String,
    pub status: String,
    pub goal: String,
    pub revision: u64,
    pub conversation: String,
    pub snapshot: Value,
    pub approved_at: Option<u64>,
    pub decided_at: Option<u64>,
    pub run_id: Option<u64>,
    #[serde(default)]
    pub execution_source: Option<String>,
    #[serde(default)]
    pub execution_goal: Option<String>,
    #[serde(default)]
    pub discussion_active: bool,
    #[serde(default)]
    pub changes: Vec<Value>,
}

pub fn invalidate(item: &mut Automation, time: u64, reason: &str) {
    for review in &mut item.confirmations {
        if ["pending", "approved", "ready"].contains(&review.status.as_str()) {
            review.status = "invalidated".into();
            review.decided_at = Some(time);
            review
                .changes
                .push(json!({"at":time,"actor":"core","reason":reason}));
        }
    }
}

fn append_message(
    connection: &Connection,
    conversation: &str,
    message: &Value,
) -> Result<(), String> {
    let data: String = connection
        .query_row(
            "SELECT COALESCE((SELECT data FROM conversations WHERE project=?1),'[]')",
            [conversation],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut history: Vec<Value> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    history.push(message.clone());
    connection.execute("INSERT INTO conversations(project,data) VALUES (?1,?2) ON CONFLICT(project) DO UPDATE SET data=excluded.data", rusqlite::params![conversation,serde_json::to_string(&history).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    Ok(())
}

fn ensure(
    item: &mut Automation,
    connection: &Connection,
    at: u64,
    trigger: &str,
    time: u64,
) -> Result<usize, String> {
    if let Some(index) = item.confirmations.iter().rposition(|r| {
        r.scheduled_at == at
            && r.trigger == trigger
            && r.status != "invalidated"
            && (trigger != "manual" || r.run_id.is_none())
    }) {
        return Ok(index);
    }
    let config = item
        .draft
        .confirmation
        .as_ref()
        .ok_or("任务未开启逐次确认")?;
    item.next_confirmation_id += 1;
    let id = item.next_confirmation_id;
    let conversation = format!("automation-confirmation:{}:{id}", item.id);
    let deadline = (trigger == "schedule" && config.on_timeout != "wait").then_some(at);
    let missed = deadline.is_some_and(|deadline| time >= deadline);
    let review = Confirmation {
        id,
        scheduled_at: at,
        requested_at: time,
        request_at: if trigger == "manual" {
            time
        } else {
            at.saturating_sub(config.before_seconds)
        },
        deadline,
        trigger: trigger.into(),
        status: if missed && config.on_timeout == "skip" {
            "expired"
        } else {
            "pending"
        }
        .into(),
        goal: item.draft.goal.clone(),
        revision: 1,
        conversation: conversation.clone(),
        snapshot: json!({"title":item.draft.title,"goal":item.draft.goal,"on_timeout":if trigger == "manual" {"wait"} else {&config.on_timeout},"project":item.draft.project,"agent_id":item.draft.agent_id,"utc_offset_seconds":item.draft.offset(),"time_window":item.draft.window(at).unwrap_or_else(|e|json!({"error":e})),"delivery_targets":item.draft.delivery_targets,"accounts":item.delivery_grants.iter().map(|g|json!({"target_id":g.target_id,"account":g.account})).collect::<Vec<_>>()}),
        approved_at: None,
        decided_at: (missed && config.on_timeout == "skip").then_some(time),
        run_id: None,
        execution_source: None,
        execution_goal: None,
        discussion_active: false,
        changes: vec![],
    };
    if !missed {
        let message = json!({"role":"assistant","content":format!("{} 即将执行，请检查本次内容，或与 Agent 沟通调整。到时按确认卡片中的策略处理。",item.draft.title),"automation_confirmation":{"automation_id":item.id,"confirmation_id":id}});
        append_message(
            connection,
            item.draft.conversation.as_deref().ok_or("缺少接收会话")?,
            &message,
        )?;
        append_message(connection, &conversation, &message)?;
        crate::notifications::insert(
            connection,
            &format!("confirmation:{}:{id}", item.id),
            "approval_pending",
            &item.draft.title,
            &conversation,
        )?;
    }
    item.confirmations.push(review);
    while item.confirmations.len() > 100 {
        let Some(index) = item.confirmations.iter().position(|r| {
            r.id != id
                && !["pending", "approved", "ready"].contains(&r.status.as_str())
                && !r.discussion_active
        }) else {
            break;
        };
        item.confirmations.remove(index);
    }
    item.confirmations
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| "确认记录丢失".into())
}

fn skip_run(item: &mut Automation, index: usize, time: u64) {
    if item.confirmations[index].run_id.is_some() {
        return;
    }
    let id = item.runs.last().map_or(1, |r| r.id + 1);
    let review = &mut item.confirmations[index];
    let reason = if review.status == "expired" {
        "计划执行时仍未确认，已跳过本次"
    } else {
        "用户跳过本次执行"
    };
    review.run_id = Some(id);
    item.runs.push(Run { id, scope: None, scheduled_at: review.scheduled_at, started_at: time, finished_at: Some(time), status: "skipped".into(), output: reason.into(), generation_output: None, skip_reason: Some(reason.into()), deliveries: vec![], context: json!({"confirmation_id":review.id,"goal":review.goal,"trigger":review.trigger,"scheduled_at":review.scheduled_at,"time_window":review.snapshot["time_window"]}) });
    while item.runs.len() > 100 {
        let Some(index) = item.runs.iter().position(|r| r.status != "running") else {
            break;
        };
        item.runs.remove(index);
    }
}

fn prepare_one(
    item: &mut Automation,
    connection: &Connection,
    at: u64,
    trigger: &str,
    time: u64,
) -> Result<bool, String> {
    let index = ensure(item, connection, at, trigger, time)?;
    let run_on_timeout = trigger == "schedule"
        && item
            .draft
            .confirmation
            .as_ref()
            .is_some_and(|c| c.on_timeout == "run");
    let review = &mut item.confirmations[index];
    if run_on_timeout && time >= at && ["pending", "approved"].contains(&review.status.as_str()) {
        let source = if review.status == "approved" && !review.discussion_active {
            "user"
        } else {
            "timeout"
        };
        let goal = if source == "user" {
            review.goal.clone()
        } else {
            review.snapshot["goal"]
                .as_str()
                .unwrap_or(&item.draft.goal)
                .to_string()
        };
        review.execution_source = Some(source.into());
        review.execution_goal = Some(goal.clone());
        review.status = "ready".into();
        review.decided_at = Some(time);
        review.changes.push(json!({"at":time,"actor":"core","action":"ready","execution_source":source,"execution_goal":goal,"revision":review.revision}));
    } else if review.status == "pending" && review.deadline.is_some_and(|deadline| time >= deadline)
    {
        review.status = "expired".into();
        review.decided_at = Some(time);
    }
    if ["expired", "skipped"].contains(&review.status.as_str())
        && (trigger == "manual" || time >= at)
    {
        skip_run(item, index, time);
        if trigger == "manual" {
            item.manual_run_at = None;
        } else {
            item.next_run_at = item.next(time);
        }
        return Ok(true);
    }
    Ok(false)
}

pub fn prepare(item: &mut Automation, connection: &Connection, time: u64) -> Result<(), String> {
    if item.status != "active" {
        return Ok(());
    }
    let Some(config) = item.draft.confirmation.clone() else {
        return Ok(());
    };
    if let Some(at) = item.manual_run_at {
        prepare_one(item, connection, at, "manual", time)?;
    }
    if let Some(at) = item
        .next_run_at
        .filter(|at| time >= at.saturating_sub(config.before_seconds))
    {
        let advanced = prepare_one(item, connection, at, "schedule", time)?;
        if advanced {
            if let Some(next) = item
                .next_run_at
                .filter(|at| time >= at.saturating_sub(config.before_seconds))
            {
                prepare_one(item, connection, next, "schedule", time)?;
            }
        }
    }
    Ok(())
}

pub fn for_execution(item: &Automation, at: u64, trigger: &str) -> Option<Confirmation> {
    item.confirmations
        .iter()
        .find(|r| {
            r.scheduled_at == at
                && r.trigger == trigger
                && ((r.status == "approved" && !r.discussion_active) || r.status == "ready")
        })
        .cloned()
        .map(|mut review| {
            if review.execution_source.is_none() {
                review.execution_source = Some("user".into());
                review.execution_goal = Some(review.goal.clone());
            }
            review
        })
}

pub fn ids(conversation: &str) -> Option<Result<(u64, u64), String>> {
    let rest = conversation.strip_prefix("automation-confirmation:")?;
    Some((|| {
        let (task, review) = rest.split_once(':').ok_or("确认会话编号无效")?;
        Ok((
            task.parse().map_err(|_| "任务编号无效")?,
            review.parse().map_err(|_| "确认编号无效")?,
        ))
    })())
}

fn mutable(item: &Automation, review: &Confirmation, time: u64) -> Result<(), String> {
    if item.status != "active" || !["pending", "approved"].contains(&review.status.as_str()) {
        return Err("本次确认已结束或失效".into());
    }
    if review.deadline.is_some_and(|at| time >= at)
        && (review.status == "pending"
            || item
                .draft
                .confirmation
                .as_ref()
                .is_some_and(|c| c.on_timeout == "run"))
    {
        return Err("本次确认已过期".into());
    }
    Ok(())
}

pub fn action_at(
    path: &Path,
    id: u64,
    review_id: u64,
    action: &str,
    args: &Value,
    time: u64,
    actor: &str,
) -> Result<Value, String> {
    let fields = args.as_object().ok_or("确认参数须为对象")?;
    let allowed: &[&str] = match action {
        "read" => &[],
        "update" => &["revision", "goal", "reason"],
        "approve" => &["revision", "apply_to_future"],
        "skip" => &["revision"],
        _ => return Err("未知确认操作".into()),
    };
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("确认操作含不支持的字段".into());
    }
    transact(path, |state, connection| {
        let item = state
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or("任务不存在")?;
        prepare(item, connection, time)?;
        let index = item
            .confirmations
            .iter()
            .position(|r| r.id == review_id)
            .ok_or("确认记录不存在或已清理")?;
        if action == "read" {
            return Ok(json!(item.confirmations[index]));
        }
        mutable(item, &item.confirmations[index], time)?;
        if actor == "agent" && action != "update" {
            return Err("Agent 只能调整本次内容，不能批准执行".into());
        }
        let revision = args["revision"].as_u64().ok_or("缺少内容版本 revision")?;
        if revision != item.confirmations[index].revision {
            return Err("内容版本已变化，请刷新并重新审阅".into());
        }
        match action {
            "update" => {
                let goal = args["goal"]
                    .as_str()
                    .filter(|g| !g.trim().is_empty() && g.chars().count() <= 40000)
                    .ok_or("本次内容须为 1–40000 个字符")?;
                let review = &mut item.confirmations[index];
                if review.goal != goal {
                    review.changes.push(json!({"at":time,"actor":actor,"previous_goal":review.goal,"goal":goal,"reason":args["reason"]}));
                    review.goal = goal.into();
                    review.revision += 1;
                    review.status = "pending".into();
                    review.approved_at = None;
                }
            }
            "approve" => {
                if item.confirmations[index].discussion_active {
                    return Err("Agent 仍在沟通调整，请等待回复后再确认".into());
                }
                let future = args
                    .get("apply_to_future")
                    .map(|v| v.as_bool().ok_or("apply_to_future 须为布尔值"))
                    .transpose()?
                    .unwrap_or(false);
                if future {
                    let goal = item.confirmations[index].goal.clone();
                    for other in &mut item.confirmations {
                        if other.id != review_id
                            && ["pending", "approved"].contains(&other.status.as_str())
                        {
                            other.status = "invalidated".into();
                            other.decided_at = Some(time);
                        }
                    }
                    item.draft.goal = goal;
                }
                let review = &mut item.confirmations[index];
                if review.status != "approved" {
                    review.changes.push(json!({"at":time,"actor":"user","action":"approved","revision":revision,"apply_to_future":future}));
                }
                review.status = "approved".into();
                review.approved_at = Some(time);
            }
            "skip" => {
                let review = &mut item.confirmations[index];
                review.status = "skipped".into();
                review.decided_at = Some(time);
            }
            _ => return Err("未知确认操作".into()),
        }
        Ok(json!(item.confirmations[index]))
    })
}

pub fn api(method: &str, automation: u64, path: &str, body: &[u8]) -> Result<Value, String> {
    let (review, action) = path.split_once('/').unwrap_or((path, "read"));
    let review = review.parse().map_err(|_| "确认编号无效")?;
    if (method == "GET" && action != "read")
        || (method != "GET" && method != "POST")
        || (method == "POST" && action == "read")
    {
        return Err("不支持的确认操作".into());
    }
    let args = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(body).map_err(|e| e.to_string())?
    };
    action_at(
        &crate::state_path(),
        automation,
        review,
        action,
        &args,
        now(),
        "user",
    )
}

pub fn discussion(conversation: &str, begin: bool) -> Result<(), String> {
    let (id, review_id) = ids(conversation).ok_or("不是执行前确认会话")??;
    transact(&crate::state_path(), |state, _| {
        let item = state
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or("任务不存在")?;
        let index = item
            .confirmations
            .iter()
            .position(|r| r.id == review_id)
            .ok_or("确认记录不存在")?;
        if begin {
            mutable(item, &item.confirmations[index], now())?;
            if item.confirmations[index].discussion_active {
                return Err("本次沟通正在执行".into());
            }
            let review = &mut item.confirmations[index];
            review.status = "pending".into();
            review.approved_at = None;
            review.discussion_active = true;
            review.revision += 1;
        } else {
            item.confirmations[index].discussion_active = false;
        }
        Ok(())
    })
}

pub fn context(conversation: &str) -> Result<(Draft, Confirmation), String> {
    let (id, review_id) = ids(conversation).ok_or("不是执行前确认会话")??;
    transact(&crate::state_path(), |state, _| {
        let item = state
            .items
            .iter()
            .find(|i| i.id == id)
            .ok_or("任务不存在")?;
        let review = item
            .confirmations
            .iter()
            .find(|r| r.id == review_id)
            .ok_or("确认记录不存在")?;
        mutable(item, review, now())?;
        Ok((item.draft.clone(), review.clone()))
    })
}

pub fn tool(id: u64, review: u64, args: &Value) -> Result<Value, String> {
    let fields = args.as_object().ok_or("参数须为对象")?;
    if fields
        .keys()
        .any(|k| !["goal", "revision", "reason"].contains(&k.as_str()))
    {
        return Err("确认工具只能操作本次目标文本".into());
    }
    if fields.is_empty() {
        context(&format!("automation-confirmation:{id}:{review}"))?;
        return action_at(
            &crate::state_path(),
            id,
            review,
            "read",
            args,
            now(),
            "agent",
        );
    }
    if args["reason"].as_str().is_none_or(|r| r.trim().is_empty()) {
        return Err("调整内容必须说明原因".into());
    }
    action_at(
        &crate::state_path(),
        id,
        review,
        "update",
        args,
        now(),
        "agent",
    )
}

pub fn descriptor() -> Value {
    json!({"name":"automation_confirmation","description":"Read this occurrence's confirmation with empty arguments, or propose a revised goal using its current revision and a reason. Only this occurrence's text can change. The user must explicitly approve revised content in the UI. Core alone applies the configured timeout policy; never execute the task or claim approval.","inputSchema":{"type":"object","properties":{"goal":{"type":"string","minLength":1,"maxLength":40000},"revision":{"type":"integer","minimum":1},"reason":{"type":"string","minLength":1}},"dependentRequired":{"goal":["revision","reason"],"revision":["goal","reason"],"reason":["goal","revision"]},"additionalProperties":false}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(policy: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let draft: Draft = serde_json::from_value(json!({"title":"Digest","goal":"Original goal","conversation":"inbox","schedule":{"kind":"interval","seconds":60},"time_window":{"kind":"lookback","seconds":30},"confirmation":{"before_seconds":10,"on_timeout":policy}})).unwrap();
        create(&path, draft, false, 0).unwrap();
        (dir, path)
    }
    fn item(path: &Path) -> Automation {
        transact(path, |s, _| Ok(s.items[0].clone())).unwrap()
    }
    fn act(
        path: &Path,
        review: u64,
        action: &str,
        args: Value,
        time: u64,
    ) -> Result<Value, String> {
        action_at(path, 1, review, action, &args, time, "user")
    }

    #[test]
    fn notifications_only_deliver_a_still_pending_confirmation_once() {
        let (_dir, path) = setup("wait");
        let store = Store::open(&path).unwrap();
        let start = store
            .notifications(
                crate::notifications::Request {
                    after: None,
                    instance: None,
                },
                None,
            )
            .unwrap();
        claim(&path, 50, 0).unwrap();
        claim(&path, 50, 0).unwrap();
        let poll = || {
            store
                .notifications(
                    crate::notifications::Request {
                        after: start["cursor"].as_i64(),
                        instance: start["instance"].as_str().map(str::to_string),
                    },
                    None,
                )
                .unwrap()
        };
        let page = poll();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["items"][0]["kind"], "approval_pending");
        assert_eq!(
            page["items"][0]["conversation"],
            "automation-confirmation:1:1"
        );
        act(&path, 1, "approve", json!({"revision":1}), 51).unwrap();
        assert_eq!(poll()["items"], json!([]));
    }

    #[test]
    fn reminder_is_transactional_once_and_approval_waits_for_original_schedule() {
        let (_dir, path) = setup("skip");
        assert!(claim(&path, 49, 4).unwrap().is_empty());
        assert!(item(&path).confirmations.is_empty());
        // Notification must not depend on Agent capacity. Repeated scans do not duplicate it.
        for _ in 0..3 {
            assert!(claim(&path, 50, 0).unwrap().is_empty());
        }
        assert_eq!(
            Store::open(&path)
                .unwrap()
                .conversation("inbox")
                .unwrap()
                .len(),
            1
        );
        act(
            &path,
            1,
            "update",
            json!({"revision":1,"goal":"Revised only once"}),
            51,
        )
        .unwrap();
        act(&path, 1, "approve", json!({"revision":2}), 52).unwrap();
        assert!(claim(&path, 59, 4).unwrap().is_empty());
        let jobs = claim(&path, 60, 4).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0.draft.goal, "Revised only once");
        let saved = item(&path);
        assert_eq!(saved.draft.goal, "Original goal");
        assert_eq!(saved.runs[0].context["goal"], "Revised only once");
        assert_eq!(
            saved.runs[0].context["time_window"],
            json!({"start":30,"end":60,"bounds":"[start,end)"})
        );
        assert_eq!(saved.confirmations[0].status, "consumed");
        assert!(claim(&path, 60, 4).unwrap().is_empty());
        assert!(act(&path, 1, "approve", json!({"revision":2}), 61).is_err());
    }

    #[test]
    fn unconfirmed_skip_records_one_audited_run_without_delivery_or_late_reminder() {
        let (_dir, path) = setup("skip");
        claim(&path, 50, 4).unwrap();
        for _ in 0..3 {
            assert!(claim(&path, 60, 4).unwrap().is_empty());
        }
        let saved = item(&path);
        assert_eq!(saved.runs.len(), 1);
        assert_eq!(saved.runs[0].status, "skipped");
        assert!(saved.runs[0].deliveries.is_empty());
        assert_eq!(saved.confirmations[0].status, "expired");
        assert_eq!(saved.next_run_at, Some(120));
        assert!(act(&path, 1, "approve", json!({"revision":1}), 61).is_err());
        // Offline cycles coalesce; do not flood the conversation on return.
        assert!(claim(&path, 600, 4).unwrap().is_empty());
        assert_eq!(item(&path).runs.len(), 2);
        assert_eq!(
            Store::open(&path)
                .unwrap()
                .conversation("inbox")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn wait_accepts_late_revision_and_preserves_window_without_accumulating_cycles() {
        let (_dir, path) = setup("wait");
        claim(&path, 500, 4).unwrap();
        assert_eq!(item(&path).confirmations.len(), 1);
        act(
            &path,
            1,
            "update",
            json!({"revision":1,"goal":"Late review"}),
            501,
        )
        .unwrap();
        assert!(act(&path, 1, "approve", json!({"revision":1}), 502).is_err());
        act(&path, 1, "approve", json!({"revision":2}), 503).unwrap();
        let jobs = claim(&path, 504, 4).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0.runs[0].scheduled_at, 60);
        assert_eq!(jobs[0].0.runs[0].context["time_window"]["end"], 60);
        assert_eq!(item(&path).next_run_at, Some(540));
    }

    #[test]
    fn manual_run_waits_even_with_skip_policy_and_keeps_periodic_slot() {
        let (_dir, path) = setup("skip");
        change(&path, 1, "run", 20).unwrap();
        claim(&path, 20, 4).unwrap();
        assert_eq!(item(&path).confirmations[0].deadline, None);
        claim(&path, 50, 4).unwrap();
        assert_eq!(item(&path).confirmations.len(), 2);
        act(&path, 1, "approve", json!({"revision":1}), 55).unwrap();
        let jobs = claim(&path, 56, 4).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0.runs[0].context["trigger"], "manual");
        assert_eq!(item(&path).next_run_at, Some(60));
        // Expiry audit cannot discard the in-flight manual run.
        claim(&path, 60, 4).unwrap();
        assert_eq!(item(&path).runs[0].status, "running");
        finish(&path, 1, 1, Ok("done".into()), 61).unwrap();
        assert_eq!(item(&path).runs[0].status, "succeeded");
    }

    #[test]
    fn goal_edits_revoke_approval_and_discussion_prevents_user_approval() {
        let (_dir, path) = setup("wait");
        claim(&path, 50, 4).unwrap();
        act(&path, 1, "approve", json!({"revision":1}), 51).unwrap();
        act(
            &path,
            1,
            "update",
            json!({"revision":1,"goal":"New content"}),
            52,
        )
        .unwrap();
        assert_eq!(item(&path).confirmations[0].status, "pending");
        transact(&path, |s, _| {
            s.items[0].confirmations[0].discussion_active = true;
            Ok(())
        })
        .unwrap();
        assert!(act(&path, 1, "approve", json!({"revision":2}), 53).is_err());
        assert!(action_at(&path, 1, 1, "approve", &json!({"revision":2}), 53, "agent").is_err());
        assert!(claim(&path, 60, 4).unwrap().is_empty());
        assert!(act(
            &path,
            1,
            "update",
            json!({"revision":2,"goal":"Other","delivery_targets":[]}),
            61
        )
        .is_err());
    }

    #[test]
    fn future_goal_is_explicit_and_pause_invalidates_old_cards() {
        let (_dir, path) = setup("wait");
        claim(&path, 50, 4).unwrap();
        act(
            &path,
            1,
            "update",
            json!({"revision":1,"goal":"Future content"}),
            51,
        )
        .unwrap();
        act(
            &path,
            1,
            "approve",
            json!({"revision":2,"apply_to_future":true}),
            52,
        )
        .unwrap();
        assert_eq!(item(&path).draft.goal, "Future content");
        change(&path, 1, "pause", 53).unwrap();
        assert_eq!(item(&path).confirmations[0].status, "invalidated");
        change(&path, 1, "resume", 54).unwrap();
        assert!(act(&path, 1, "approve", json!({"revision":2}), 55).is_err());
        assert!(claim(&path, 60, 4).unwrap().is_empty());
    }

    #[test]
    fn skipping_before_due_does_not_execute_and_legacy_tasks_stay_ungated() {
        let (_dir, path) = setup("wait");
        claim(&path, 50, 4).unwrap();
        act(&path, 1, "skip", json!({"revision":1}), 51).unwrap();
        assert!(claim(&path, 60, 4).unwrap().is_empty());
        assert_eq!(item(&path).runs[0].status, "skipped");
        transact(&path, |s, _| {
            s.items[0].draft.confirmation = None;
            Ok(())
        })
        .unwrap();
        assert_eq!(claim(&path, 120, 4).unwrap().len(), 1);
    }

    #[test]
    fn two_completed_manual_requests_in_one_second_have_distinct_confirmations() {
        let (_dir, path) = setup("wait");
        for id in 1..=2 {
            change(&path, 1, "run", 20).unwrap();
            claim(&path, 20, 4).unwrap();
            assert_eq!(item(&path).confirmations.last().unwrap().id, id);
            act(&path, id, "approve", json!({"revision":1}), 20).unwrap();
            assert_eq!(claim(&path, 20, 4).unwrap().len(), 1);
            finish(&path, 1, id, Ok("done".into()), 20).unwrap();
        }
        assert_eq!(item(&path).runs.len(), 2);
    }

    #[test]
    fn pending_manual_review_does_not_block_an_approved_periodic_occurrence() {
        let (_dir, path) = setup("wait");
        change(&path, 1, "run", 20).unwrap();
        claim(&path, 50, 4).unwrap();
        act(&path, 2, "approve", json!({"revision":1}), 55).unwrap();
        let jobs = claim(&path, 60, 4).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0.runs[0].context["trigger"], "schedule");
        assert_eq!(item(&path).manual_run_at, Some(20));
        finish(&path, 1, 1, Ok("scheduled done".into()), 61).unwrap();
        act(&path, 1, "approve", json!({"revision":1}), 62).unwrap();
        assert_eq!(
            claim(&path, 62, 4).unwrap()[0].0.runs[1].context["trigger"],
            "manual"
        );
        assert_eq!(item(&path).next_run_at, Some(120));
    }

    #[test]
    fn run_policy_executes_at_due_time_without_claiming_user_approval() {
        let (_dir, path) = setup("run");
        assert!(claim(&path, 50, 4).unwrap().is_empty());
        assert!(claim(&path, 59, 4).unwrap().is_empty());
        let jobs = claim(&path, 60, 4).unwrap();
        assert_eq!(jobs.len(), 1);
        let saved = item(&path);
        assert_eq!(saved.runs[0].context["goal"], "Original goal");
        assert_eq!(
            saved.runs[0].context["confirmation"]["execution_source"],
            "timeout"
        );
        assert_eq!(saved.runs[0].context["time_window"]["end"], 60);
        assert_eq!(
            saved.confirmations[0].execution_source.as_deref(),
            Some("timeout")
        );
        assert!(saved.confirmations[0].approved_at.is_none());
        assert_eq!(saved.confirmations[0].status, "consumed");
        assert!(claim(&path, 60, 4).unwrap().is_empty());
    }

    #[test]
    fn run_policy_freezes_original_goal_during_discussion_even_when_capacity_is_full() {
        let (_dir, path) = setup("run");
        claim(&path, 50, 4).unwrap();
        act(
            &path,
            1,
            "update",
            json!({"revision":1,"goal":"Unapproved proposal"}),
            51,
        )
        .unwrap();
        transact(&path, |s, _| {
            s.items[0].confirmations[0].discussion_active = true;
            Ok(())
        })
        .unwrap();
        assert!(claim(&path, 60, 0).unwrap().is_empty());
        let saved = item(&path);
        assert_eq!(saved.confirmations[0].status, "ready");
        assert_eq!(
            saved.confirmations[0].execution_goal.as_deref(),
            Some("Original goal")
        );
        assert_eq!(saved.confirmations[0].goal, "Unapproved proposal");
        assert!(act(
            &path,
            1,
            "update",
            json!({"revision":2,"goal":"Late changes"}),
            61
        )
        .is_err());
        assert!(act(&path, 1, "approve", json!({"revision":2}), 61).is_err());
        assert!(act(&path, 1, "skip", json!({"revision":2}), 61).is_err());
        // Reopening persistent state cannot change the selected goal or original window.
        let jobs = claim(&path, 500, 4).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0.draft.goal, "Original goal");
        assert_eq!(jobs[0].0.runs[0].scheduled_at, 60);
        assert_eq!(jobs[0].0.runs[0].context["time_window"]["start"], 30);
        assert_eq!(item(&path).confirmations[0].decided_at, Some(60));
        assert_eq!(item(&path).next_run_at, Some(540));
    }

    #[test]
    fn run_policy_uses_explicitly_approved_changes_but_not_revoked_approval() {
        for edit_again in [false, true] {
            let (_dir, path) = setup("run");
            claim(&path, 50, 4).unwrap();
            act(
                &path,
                1,
                "update",
                json!({"revision":1,"goal":"Approved changes"}),
                51,
            )
            .unwrap();
            act(&path, 1, "approve", json!({"revision":2}), 52).unwrap();
            if edit_again {
                act(
                    &path,
                    1,
                    "update",
                    json!({"revision":2,"goal":"New proposal"}),
                    53,
                )
                .unwrap();
            }
            assert!(claim(&path, 59, 4).unwrap().is_empty());
            let jobs = claim(&path, 60, 4).unwrap();
            assert_eq!(
                jobs[0].0.draft.goal,
                if edit_again {
                    "Original goal"
                } else {
                    "Approved changes"
                }
            );
            assert_eq!(
                item(&path).confirmations[0].execution_source.as_deref(),
                Some(if edit_again { "timeout" } else { "user" })
            );
        }
    }

    #[test]
    fn run_policy_honors_explicit_skip_and_pause_invalidates_a_queued_decision() {
        let (_dir, path) = setup("run");
        claim(&path, 50, 4).unwrap();
        act(&path, 1, "skip", json!({"revision":1}), 51).unwrap();
        assert!(claim(&path, 60, 4).unwrap().is_empty());
        assert_eq!(item(&path).runs[0].status, "skipped");
        claim(&path, 120, 0).unwrap();
        assert_eq!(item(&path).confirmations.last().unwrap().status, "ready");
        change(&path, 1, "pause", 121).unwrap();
        assert_eq!(
            item(&path).confirmations.last().unwrap().status,
            "invalidated"
        );
        assert!(claim(&path, 122, 4).unwrap().is_empty());
    }

    #[test]
    fn run_policy_manual_requests_still_wait_and_offline_cycles_coalesce_without_reminders() {
        let (_dir, path) = setup("run");
        change(&path, 1, "run", 20).unwrap();
        assert!(claim(&path, 20, 4).unwrap().is_empty());
        assert_eq!(item(&path).confirmations[0].snapshot["on_timeout"], "wait");
        let jobs = claim(&path, 500, 4).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0.runs[0].context["trigger"], "schedule");
        assert_eq!(item(&path).manual_run_at, Some(20));
        assert_eq!(item(&path).next_run_at, Some(540));
        assert_eq!(
            Store::open(&path)
                .unwrap()
                .conversation("inbox")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn configuration_validates_and_confirmation_defaults_off_for_legacy_records() {
        assert!(serde_json::from_value::<ConfirmationConfig>(
            json!({"before_seconds":600,"on_timeout":"run"})
        )
        .unwrap()
        .validate()
        .is_ok());
        assert!(serde_json::from_value::<ConfirmationConfig>(
            json!({"before_seconds":600,"on_timeout":"other"})
        )
        .unwrap()
        .validate()
        .is_err());
        for before_seconds in [0, 604801] {
            assert!(serde_json::from_value::<ConfirmationConfig>(
                json!({"before_seconds":before_seconds})
            )
            .unwrap()
            .validate()
            .is_err());
        }
        assert!(
            serde_json::from_value::<ConfirmationConfig>(json!({"before_seconds":600}))
                .unwrap()
                .validate()
                .is_ok()
        );
        assert!(serde_json::from_value::<ConfirmationConfig>(
            json!({"before_seconds":600,"extra":true})
        )
        .is_err());
    }
}
