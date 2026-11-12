//! One task-scoped model attempt, with durable reservation and owner review.
use super::*;
use serde_json::{Value, json};
use supa_persistence::{SupervisedCommand, SupervisedInput};
use supa_providers::controlled::ControlledProvider;

impl Runtime {
    pub async fn supervised_reasoning_with_provider(
        &self,
        actor: Uuid,
        task: Uuid,
        command: &SupervisedCommand,
        provider: &dyn ControlledProvider,
    ) -> Result<Value> {
        command.validate()?;
        let current = self
            .store
            .supervised(actor, "get", json!({"task_id":task}))
            .await?;
        let input: SupervisedInput =
            serde_json::from_value(current["input"].clone()).map_err(|_| Error::Invalid)?;
        input.validate()?;
        let plan = input.reasoning.as_ref().ok_or(Error::Denied)?;
        plan.validate_dispatch()?;
        let real = plan.real.as_ref();
        if command.operation != "advance"
            || provider.work_evidence_kind() != plan.provider
            || provider.project()
                != real
                    .map(|s| s.project.as_str())
                    .unwrap_or("local-synthetic")
            || serde_json::to_value(provider.policy()).map_err(|_| Error::Invalid)?
                != serde_json::to_value(plan.policy()?).map_err(|_| Error::Invalid)?
        {
            return Err(Error::Denied);
        }
        if current["inference"].is_object() {
            // A lost response or process restart NEVER reissues a reserved call.
            if current["inference"]["request_id"] == json!(command.request_id) {
                if current["inference"]["request_body"]
                    != serde_json::to_value(command).map_err(|_| Error::Invalid)?
                {
                    return Err(Error::Stale);
                }
                return Ok(current);
            }
            return Err(Error::Denied);
        }
        if current["step"] != 1 {
            return Err(Error::Denied);
        }
        let packet = reasoning_packet(plan, &input.task, &input.desired_outcome, &input.material)?;
        let reservation_id = Uuid::new_v4();
        let mut body = serde_json::to_value(command).map_err(|_| Error::Invalid)?;
        body["task_id"] = json!(task);
        body["reservation_id"] = json!(reservation_id);
        let reserved = self
            .store
            .supervised(actor, "reserve_inference", body)
            .await?;
        let revision = reserved["revision"].clone();
        let guard = TaskDispatchGuard {
            store: self.store.clone(),
            actor,
            task,
            revision: revision.clone(),
            plan: plan.clone(),
        };
        let call = provider.supervised_work(&packet, &guard);
        tokio::pin!(call);
        let timeout = tokio::time::sleep(if real.is_some() {
            Duration::from_secs(25)
        } else {
            Duration::from_millis(1500)
        });
        tokio::pin!(timeout);
        let mut interval = tokio::time::interval(Duration::from_millis(75));
        let invocation = loop {
            tokio::select! {
                result=&mut call=>break result,
                _=&mut timeout=>break Invocation {result:Err(Error::Timeout),usage:None,diagnostic:None},
                _=interval.tick()=>{
                    let now=self.store.supervised(actor,"get",json!({"task_id":task})).await;
                    if !now.as_ref().is_ok_and(|t|t["state"]=="running" && t["revision"]==revision) {
                        break Invocation {result:Err(Error::Denied),usage:None,diagnostic:None};
                    }
                }
            }
        };
        let usage = invocation.usage;
        let result = invocation.result.and_then(|report| {
            validate_reasoning_result(plan, &packet, &report, usage.ok_or(Error::Invalid)?)?;
            Ok(report)
        });
        let payload = result.as_ref().ok().map(|report| {
            json!({"action":"reasoning_result",
            "report":report,"provider":plan.provider,"model":plan.model,"usage":usage,
            "external_requests":if real.is_some(){3}else{0},"inference_attempts":1,
            "spending_microusd":usage.and_then(|u|plan.policy().ok()?.cost(u).ok()),
            "mock_model":real.is_none(),
            "interpretations_are_unverified":true,"owner_review_required":true})
        });
        let settled=self.store.supervised(actor,"settle_inference",json!({"task_id":task,
            "reservation_id":reservation_id,"diagnostic":invocation.diagnostic.filter(|d|d.validate().is_ok()),"usage":usage,"outcome":if result.is_ok(){"accepted"}else{"rejected"},"payload":payload})).await?;
        if settled.is_null() {
            return Err(Error::Denied);
        }
        Ok(settled)
    }
}

pub(super) fn reviewed_reasoning_memory(task: &Value) -> Result<Value> {
    let input: SupervisedInput =
        serde_json::from_value(task["input"].clone()).map_err(|_| Error::Invalid)?;
    input.reasoning.as_ref().ok_or(Error::Denied)?.validate()?;
    let report: WorkProposal = serde_json::from_value(task["payloads"][1]["report"].clone())
        .map_err(|_| Error::Invalid)?;
    let event = task["events"]
        .as_array()
        .and_then(|e| e.last())
        .ok_or(Error::Invalid)?;
    // Review is not external verification. Memory remains a private hypothesis.
    let memory = if input.retain_memory {
        json!({"id":Uuid::new_v4(),"kind":"hypothesis",
        "lesson":report.interpretation,"source_task_id":task["id"],"source_event_id":event["id"],
        "source_input_sha256":task["input_sha256"],"source_result_sha256":task["payloads"][1]["result_sha256"],"created_at":chrono::Utc::now()})
    } else {
        Value::Null
    };
    Ok(
        json!({"action":"retain_and_assess","memory":memory,"outcome":"owner_reviewed",
        "goal_assessment":{"desired_outcome":input.desired_outcome,"assessment":report.summary,
        "uncertainty":report.missing_information,"agent_goal_completion_changed":false,"owner_reviewed":true},
        "external_requests":0,"spending_microusd":0}),
    )
}

struct TaskDispatchGuard {
    store: supa_persistence::Store,
    actor: Uuid,
    task: Uuid,
    revision: Value,
    plan: ReasoningPlan,
}
impl supa_providers::controlled::ControlledDispatchGuard for TaskDispatchGuard {
    fn authorize(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            self.plan.validate_dispatch()?;
            let t = self
                .store
                .supervised(self.actor, "get", json!({"task_id":self.task}))
                .await?;
            if t["state"] != "running" || t["revision"] != self.revision {
                return Err(Error::Denied);
            }
            let now = chrono::Utc::now();
            for field in ["approved_until", "deadline"] {
                let expires =
                    chrono::DateTime::parse_from_rfc3339(t[field].as_str().ok_or(Error::Denied)?)
                        .map_err(|_| Error::Denied)?;
                if expires <= now {
                    return Err(Error::Denied);
                }
            }
            Ok(())
        })
    }
}

/// Independent server-side authorization, never accepted from a task command.
/// The SQL budget UUID is unique for the lifetime of the ledger: restarting or
/// changing a task cannot recycle this cumulative spending authorization.
pub(super) fn authorize_paid_scope(
    plan: &ReasoningPlan,
    actor: Uuid,
    task: Uuid,
    input_sha256: &str,
    raw: Option<&str>,
) -> Result<()> {
    plan.validate_dispatch()?;
    let scope = plan.real.as_ref().ok_or(Error::Denied)?;
    let receipt: Value =
        serde_json::from_str(raw.filter(|s| s.len() <= 2048).ok_or(Error::Denied)?)
            .map_err(|_| Error::Denied)?;
    let fields = [
        "owner_id",
        "task_id",
        "input_sha256",
        "budget_id",
        "project",
        "model",
        "expires_at",
        "per_task_microusd",
        "cumulative_microusd",
    ];
    let object = receipt.as_object().ok_or(Error::Denied)?;
    if object.len() != fields.len() || fields.iter().any(|f| !object.contains_key(*f)) {
        return Err(Error::Denied);
    }
    let expires =
        chrono::DateTime::parse_from_rfc3339(receipt["expires_at"].as_str().ok_or(Error::Denied)?)
            .map_err(|_| Error::Denied)?
            .with_timezone(&chrono::Utc);
    if actor.is_nil()
        || task.is_nil()
        || receipt["owner_id"] != json!(actor)
        || receipt["task_id"] != json!(task)
        || input_sha256.len() != 64
        || !input_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || receipt["input_sha256"] != json!(input_sha256)
        || receipt["budget_id"] != json!(scope.budget_id)
        || receipt["project"] != json!(scope.project)
        || receipt["model"] != json!(plan.model)
        || expires != scope.expires_at
        || expires <= chrono::Utc::now()
        || receipt["per_task_microusd"].as_i64().ok_or(Error::Denied)? < plan.max_cost_microusd
        || receipt["cumulative_microusd"]
            .as_i64()
            .ok_or(Error::Denied)?
            < plan.max_cost_microusd
    {
        return Err(Error::Denied);
    }
    Ok(())
}

#[cfg(test)]
mod spending_tests {
    use super::*;
    #[test]
    fn paid_approval_is_separate_exact_expiring_and_budget_bound() {
        let owner = Uuid::new_v4();
        let task = Uuid::new_v4();
        let hash = "a".repeat(64);
        let expires = chrono::Utc::now() + chrono::Duration::minutes(5);
        let budget = Uuid::new_v4();
        let plan:ReasoningPlan=serde_json::from_value(json!({"provider":"openai","model":"gpt-6-luna",
            "max_input_tokens":2000,"max_output_tokens":256,"max_requests":1,"max_cost_microusd":378,"external_tools":[],
            "real":{"project":"proj_synthetic","budget_id":budget,"expires_at":expires}})).unwrap();
        let receipt = json!({"owner_id":owner,"task_id":task,"input_sha256":hash,"budget_id":budget,
            "project":"proj_synthetic","model":"gpt-6-luna","expires_at":expires,
            "per_task_microusd":378,"cumulative_microusd":378});
        assert!(authorize_paid_scope(&plan, owner, task, &hash, None).is_err());
        assert!(
            authorize_paid_scope(&plan, owner, task, &hash, Some(&receipt.to_string())).is_ok()
        );
        for (key, value) in [
            ("owner_id", json!(Uuid::new_v4())),
            ("task_id", json!(Uuid::new_v4())),
            ("input_sha256", json!("b".repeat(64))),
            ("budget_id", json!(Uuid::new_v4())),
            ("project", json!("proj_wrong")),
            ("model", json!("other")),
            (
                "expires_at",
                json!(chrono::Utc::now() - chrono::Duration::seconds(1)),
            ),
            ("per_task_microusd", json!(377)),
            ("cumulative_microusd", json!(377)),
            ("unexpected", json!(true)),
        ] {
            let mut bad = receipt.clone();
            bad[key] = value;
            assert!(
                authorize_paid_scope(&plan, owner, task, &hash, Some(&bad.to_string())).is_err(),
                "{key}"
            );
        }
    }
}
