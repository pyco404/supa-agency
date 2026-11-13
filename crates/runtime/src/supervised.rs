//! Finite request-driven task steps; real inference uses only the separately gated
//! task adapter. Never invokes autonomous cycles, wallets, schedulers or publishing.
use super::*;
use serde_json::{Value, json};
use std::time::Instant;
use supa_persistence::{SupervisedCommand, SupervisedInput};
impl Runtime {
    pub async fn supervised_command(
        &self,
        actor: Uuid,
        task: Uuid,
        command: &SupervisedCommand,
    ) -> Result<Value> {
        command.validate()?;
        let mut body = serde_json::to_value(command).map_err(|_| Error::Invalid)?;
        body["task_id"] = json!(task);
        if command.operation == "review" {
            let current = self
                .store
                .supervised(actor, "get", json!({"task_id":task}))
                .await?;
            body["payload"] = crate::supervised_reasoning::reviewed_reasoning_memory(&current)?;
        }
        if command.operation == "advance" {
            let current = self
                .store
                .supervised(actor, "get", json!({"task_id":task}))
                .await?;
            if current["input"]["reasoning"].is_object() && current["step"] != 0 {
                if current["input"]["reasoning"]["real"].is_object() {
                    // One operator-approved evaluation identity and task, never a global AI switch.
                    if std::env::var("SUPA_SUPERVISED_REAL_TASK_ID")
                        .ok()
                        .as_deref()
                        != Some(task.to_string().as_str())
                        || std::env::var("SUPA_SUPERVISED_REAL_OWNER_ID")
                            .ok()
                            .as_deref()
                            != Some(actor.to_string().as_str())
                    {
                        return Err(Error::Denied);
                    }
                    let plan: ReasoningPlan =
                        serde_json::from_value(current["input"]["reasoning"].clone())
                            .map_err(|_| Error::Invalid)?;
                    // Task approval is not spending approval. A separately installed,
                    // expiring one-budget receipt must bind the exact stored input.
                    crate::supervised_reasoning::authorize_paid_scope(
                        &plan,
                        actor,
                        task,
                        current["input_sha256"].as_str().ok_or(Error::Denied)?,
                        std::env::var("SUPA_SUPERVISED_SPENDING_APPROVAL")
                            .ok()
                            .as_deref(),
                    )?;
                    let provider =
                        supa_providers::supervised_openai::SupervisedOpenAi::from_env(&plan)?;
                    return self
                        .supervised_reasoning_with_provider(actor, task, command, &provider)
                        .await;
                }
                return self
                    .supervised_reasoning_with_provider(
                        actor,
                        task,
                        command,
                        &supa_providers::supervised_mock::SupervisedMock,
                    )
                    .await;
            }
            // CAS in the database rechecks revision and ownership after computation;
            // a concurrent pause, cancellation or revocation prevents any commit.
            body["payload"] = supervised_payload(&current)?;
            if current["step"] == 0 {
                let agent_id: Uuid = serde_json::from_value(current["agent_id"].clone())
                    .map_err(|_| Error::Invalid)?;
                let owned = self.store.factory_owned_agent(actor, agent_id).await?;
                let mut memories = vec![];
                if owned["configuration"]["requested_capabilities"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|c| c == "read_memory"))
                {
                    let previous = self
                        .store
                        .supervised(actor, "list", json!({"agent_id":agent_id}))
                        .await?;
                    for task in previous["tasks"].as_array().ok_or(Error::Database)? {
                        let m = &task["payloads"][2]["memory"];
                        if m.is_object() {
                            memories.push(Memory {
                                id: serde_json::from_value(m["id"].clone())
                                    .map_err(|_| Error::Database)?,
                                lesson: m["lesson"].as_str().ok_or(Error::Database)?.into(),
                                kind: MemoryKind::Hypothesis,
                                source_event_ids: vec![
                                    serde_json::from_value(m["source_event_id"].clone())
                                        .map_err(|_| Error::Database)?,
                                ],
                                cycle_id: serde_json::from_value(m["source_task_id"].clone())
                                    .map_err(|_| Error::Database)?,
                                created_at: serde_json::from_value(m["created_at"].clone())
                                    .map_err(|_| Error::Database)?,
                            });
                        }
                    }
                }
                let personality: Personality =
                    serde_json::from_value(owned["configuration"]["personality"].clone())
                        .map_err(|_| Error::Database)?;
                let mut projection = Agent::new(
                    "Private supervised memory projection".into(),
                    personality,
                    CycleConfig::default(),
                    Default::default(),
                )?;
                projection.memory = memories;
                // Existing relevance ranking; the adapter's cycle IDs denote source
                // task IDs, never a claim that autonomous cycles were executed.
                let relevant = relevant_memories(
                    &projection,
                    current["input"]["task"].as_str().ok_or(Error::Invalid)?,
                    4,
                );
                body["payload"]["retrieved_task_memories"]=json!(relevant.iter().map(|m|json!({"id":m.id,"lesson":m.lesson,"kind":"hypothesis","source_task_id":m.cycle_id,"source_event_ids":m.source_event_ids})).collect::<Vec<_>>());
            }
        }
        self.store.supervised(actor, &command.operation, body).await
    }
}
pub(crate) fn supervised_payload(task: &Value) -> Result<Value> {
    let start = Instant::now();
    let input: SupervisedInput =
        serde_json::from_value(task["input"].clone()).map_err(|_| Error::Invalid)?;
    input.validate()?;
    let work = WorkRequest {
        objective: input.task.clone(),
        material: input.material.clone(),
        requirements: vec![input.desired_outcome.clone()],
        require_external_research: false,
    };
    let payload = match task["step"].as_u64().ok_or(Error::Invalid)? {
        0 => {
            json!({"action":"inspect_input","material_bytes":input.material.len(),"source":"owner_supplied_synthetic_material","external_requests":0,"spending_microusd":0})
        }
        1 => {
            json!({"action":"extract_evidence","report":analyze_work(&work,ServiceKind::Summary)?,"external_requests":0,"spending_microusd":0})
        }
        2 => {
            let report = &task["payloads"][1]["report"];
            if report["contract"] != "bounded_work_v1" {
                return Err(Error::Invalid);
            }
            let evidence_id = task["events"]
                .as_array()
                .and_then(|e| e.last())
                .and_then(|e| e["id"].as_str())
                .ok_or(Error::Invalid)?;
            let summary = report["evidence"]
                .as_array()
                .and_then(|e| e.first())
                .and_then(|e| e["quote"].as_str())
                .unwrap_or("No extractable supplied statement.");
            // Hypothesis: supplied text is not promoted to independently verified fact.
            let memory = if input.retain_memory {
                json!({"id":Uuid::new_v4(),"kind":"hypothesis","lesson":summary,"source_task_id":task["id"],"source_event_id":evidence_id,"source_input_sha256":task["input_sha256"],"created_at":chrono::Utc::now()})
            } else {
                Value::Null
            };
            json!({"action":"retain_and_assess","memory":memory,"outcome":report["status"],"goal_assessment":{"literal_evidence_present":report["evaluation"]["missing_requirements"]==0,"method":"literal supplied-text coverage; not semantic verification","agent_goal_completion_changed":false,"owner_review_required":true},"external_requests":0,"spending_microusd":0})
        }
        // Idempotent replay after completion is resolved by the durable command key.
        3 => Value::Null,
        _ => return Err(Error::Invalid),
    };
    if start.elapsed() > Duration::from_millis(250) {
        return Err(Error::Timeout);
    }
    Ok(payload)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_work_reuses_existing_evidence_engine() {
        let t = json!({"step":1,"input":{"task":"Summarize synthetic status","desired_outcome":"reviewed","material":"Synthetic report reviewed. No external action.","synthetic_only":true,"retain_memory":false}});
        let p = supervised_payload(&t).unwrap();
        assert_eq!(p["report"]["status"], "completed");
        assert_eq!(
            p["report"]["evidence"][0]["classification"],
            "unverified_supplied_statement"
        );
        assert_eq!(p["spending_microusd"], 0);
    }
    #[test]
    fn secrets_and_real_inputs_fail_closed() {
        for material in [
            "password=private".to_owned(),
            ["-----BEGIN ", "PRIVATE KEY-----"].concat(),
        ] {
            let t = json!({"step":1,"input":{"task":"Summary","desired_outcome":"report","material":material,"synthetic_only":true,"retain_memory":false}});
            assert!(supervised_payload(&t).is_err());
        }
    }
}
