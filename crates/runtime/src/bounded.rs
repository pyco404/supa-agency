//! Reuses the deterministic supervised evidence engine. No provider, network or financial path.
use super::*;
use serde_json::Value;
impl Runtime {
    /// At most one claimed cycle and three bounded actions; errors are returned without retry.
    /// A crash consumes its durable allowance and leaves a lease for quarantine, never replay.
    pub async fn bounded_tick(&self) -> Result<Option<Value>> {
        tokio::time::timeout(Duration::from_secs(10), self.bounded_cycle())
            .await
            .map_err(|_| Error::Timeout)?
    }
    async fn bounded_cycle(&self) -> Result<Option<Value>> {
        let Some(claim) = self.store.bounded_claim().await? else {
            return Ok(None);
        };
        let run: Uuid =
            serde_json::from_value(claim["run_id"].clone()).map_err(|_| Error::Database)?;
        let lease: Uuid =
            serde_json::from_value(claim["lease_id"].clone()).map_err(|_| Error::Database)?;
        let mut task = claim["task"].clone();
        for _ in 0..3 {
            // Defense in depth: scheduled input may never enter the real or mock reasoning adapter.
            if task["input"]["reasoning"].is_object() || task["input"]["retain_memory"] != false {
                return Err(Error::Denied);
            }
            let mut payload = crate::supervised::supervised_payload(&task)?;
            if task["step"] == 0 {
                let mut projection = Agent::new(
                    "Private bounded memory projection".into(),
                    Personality::default(),
                    CycleConfig::default(),
                    Default::default(),
                )?;
                for memory in claim["memories"].as_array().ok_or(Error::Database)? {
                    projection.memory.push(Memory {
                        id: serde_json::from_value(memory["id"].clone())
                            .map_err(|_| Error::Database)?,
                        lesson: memory["lesson"].as_str().ok_or(Error::Database)?.into(),
                        kind: MemoryKind::Hypothesis,
                        source_event_ids: vec![
                            serde_json::from_value(memory["source_event_id"].clone())
                                .map_err(|_| Error::Database)?,
                        ],
                        cycle_id: serde_json::from_value(memory["source_task_id"].clone())
                            .map_err(|_| Error::Database)?,
                        created_at: serde_json::from_value(memory["created_at"].clone())
                            .map_err(|_| Error::Database)?,
                    });
                }
                payload["retrieved_task_memories"] = serde_json::json!(relevant_memories(
                    &projection,
                    task["input"]["task"].as_str().ok_or(Error::Database)?,
                    4
                ));
            }
            let revision = task["revision"]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or(Error::Database)?;
            task = self
                .store
                .bounded_advance(run, lease, revision, payload)
                .await?;
        }
        if task["state"] != "completed" {
            return Err(Error::Denied);
        }
        Ok(Some(task))
    }
}
