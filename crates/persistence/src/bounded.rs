//! Finite synthetic goal scopes. Separate SQL grants; frozen Factory agents stay frozen.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundedScope {
    pub goal_id: Uuid,
    pub priority: u8,
    pub input: SupervisedInput,
    pub max_cycles: u8,
    pub max_actions: u8,
    pub max_spending_microusd: u64,
    pub interval_seconds: u32,
    pub valid_for_seconds: u32,
    pub mode: String,
}
impl BoundedScope {
    pub fn validate(&self) -> Result<()> {
        self.input.validate()?;
        if self.goal_id.is_nil()
            || self.priority > 5
            || !(1..=10).contains(&self.max_cycles)
            || !(3..=30).contains(&self.max_actions)
            || u16::from(self.max_cycles) * 3 > u16::from(self.max_actions)
            || self.max_spending_microusd != 0
            || !(5..=86400).contains(&self.interval_seconds)
            || !(60..=3600).contains(&self.valid_for_seconds)
            || self.mode != "deterministic"
            || self.input.reasoning.is_some()
            || self.input.retain_memory
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundedCreate {
    pub request_id: Uuid,
    pub scope: BoundedScope,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundedCommand {
    pub request_id: Uuid,
    pub operation: String,
    pub expected_revision: u32,
    pub scope_sha256: String,
    pub confirmed: bool,
    pub task_id: Option<Uuid>,
    pub result_sha256: Option<String>,
    pub accept: Option<bool>,
    pub retain_memory: Option<bool>,
}
impl BoundedCommand {
    pub fn validate(&self) -> Result<()> {
        let hash = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if self.request_id.is_nil()
            || !hash(&self.scope_sha256)
            || !["approve", "start", "pause", "resume", "stop", "review"]
                .contains(&self.operation.as_str())
            || (matches!(
                self.operation.as_str(),
                "approve" | "start" | "resume" | "review"
            ) && !self.confirmed)
        {
            return Err(Error::Denied);
        }
        if self.operation == "review" {
            if self.task_id.is_none_or(|id| id.is_nil())
                || !self.result_sha256.as_deref().is_some_and(hash)
                || self.accept.is_none()
                || self.retain_memory.is_none()
                || (self.retain_memory == Some(true) && self.accept != Some(true))
            {
                return Err(Error::Denied);
            }
        } else if self.task_id.is_some()
            || self.result_sha256.is_some()
            || self.accept.is_some()
            || self.retain_memory.is_some()
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
impl Store {
    pub async fn verify_bounded_schema(&self, execution: bool) -> Result<()> {
        let function = if execution {
            "agent_bounded.claim()"
        } else {
            "agent_bounded.command(uuid,text,jsonb)"
        };
        let valid: bool = sqlx::query_scalar("SELECT has_schema_privilege(current_user,'agent_bounded','USAGE') AND has_function_privilege(current_user,$1,'EXECUTE') AND NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=current_user AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolbypassrls)) AND NOT has_schema_privilege(current_user,'agent_bounded','CREATE') AND CASE WHEN $2 THEN has_function_privilege(current_user,'agent_bounded.advance(uuid,uuid,integer,jsonb)','EXECUTE') AND NOT has_function_privilege(current_user,'agent_bounded.command(uuid,text,jsonb)','EXECUTE') ELSE NOT has_function_privilege(current_user,'agent_bounded.claim()','EXECUTE') AND NOT has_function_privilege(current_user,'agent_bounded.advance(uuid,uuid,integer,jsonb)','EXECUTE') END AND NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('agent_bounded','agent_supervised') AND c.relkind='r' AND has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE'))")
            .bind(function).bind(execution).fetch_one(&self.pool).await.map_err(db)?;
        if !valid {
            return Err(Error::Denied);
        }
        Ok(())
    }
    pub async fn bounded(&self, actor: Uuid, operation: &str, body: Value) -> Result<Value> {
        if actor.is_nil() || self.operational.is_some() {
            return Err(Error::Denied);
        }
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("SET LOCAL lock_timeout='1s'")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let result = sqlx::query_scalar("SELECT agent_bounded.command($1,$2,$3)")
            .bind(actor)
            .bind(operation)
            .bind(body)
            .fetch_one(&mut *tx)
            .await
            .map_err(bounded_error)?;
        tx.commit().await.map_err(db)?;
        Ok(result)
    }
    /// Execution-only login: bounded transactions never wait indefinitely on a database lock.
    pub async fn bounded_claim(&self) -> Result<Option<Value>> {
        let mut tx = self.bounded_transaction().await?;
        let value = sqlx::query_scalar("SELECT agent_bounded.claim()")
            .fetch_one(&mut *tx)
            .await
            .map_err(bounded_error)?;
        tx.commit().await.map_err(db)?;
        Ok(value)
    }
    async fn bounded_transaction(&self) -> Result<Tx<'_>> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("SET LOCAL lock_timeout='1s'")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        Ok(tx)
    }
    pub async fn bounded_advance(
        &self,
        run: Uuid,
        lease: Uuid,
        revision: i32,
        payload: Value,
    ) -> Result<Value> {
        let mut tx = self.bounded_transaction().await?;
        let value = sqlx::query_scalar("SELECT agent_bounded.advance($1,$2,$3,$4)")
            .bind(run)
            .bind(lease)
            .bind(revision)
            .bind(payload)
            .fetch_one(&mut *tx)
            .await
            .map_err(bounded_error)?;
        tx.commit().await.map_err(db)?;
        Ok(value)
    }
}
fn bounded_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("42501") => Error::Denied,
        Some("40001" | "23505") => Error::Stale,
        Some("22023" | "22P02" | "23514" | "22003") => Error::Invalid,
        Some("54000") => Error::ResourceBudget,
        Some("57014" | "55P03") => Error::Timeout,
        _ => Error::Database,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> BoundedScope {
        BoundedScope {
            goal_id: Uuid::new_v4(),
            priority: 0,
            input: SupervisedInput {
                task: "Review synthetic notes".into(),
                desired_outcome: "Supported observations".into(),
                material: "Synthetic service recovered after restart.".into(),
                synthetic_only: true,
                retain_memory: false,
                reasoning: None,
            },
            max_cycles: 3,
            max_actions: 9,
            max_spending_microusd: 0,
            interval_seconds: 5,
            valid_for_seconds: 60,
            mode: "deterministic".into(),
        }
    }
    #[test]
    fn bounded_scopes_never_authorize_paid_or_unreviewed_memory() {
        scope().validate().unwrap();
        let mut s = scope();
        s.input.retain_memory = true;
        assert!(s.validate().is_err());
        let mut s = scope();
        s.max_spending_microusd = 1;
        assert!(s.validate().is_err());
        let mut s = scope();
        s.max_actions = 8;
        assert!(s.validate().is_err());
        let mut s = scope();
        s.max_cycles = 11;
        assert!(s.validate().is_err());
        let mut s = scope();
        s.valid_for_seconds = 3601;
        assert!(s.validate().is_err());
        let mut s = scope();
        s.interval_seconds = 4;
        assert!(s.validate().is_err());
        let mut s = scope();
        s.mode = "openai".into();
        assert!(s.validate().is_err());
        let mut s = serde_json::to_value(scope()).unwrap();
        s["tools"] = json!(["shell"]);
        assert!(serde_json::from_value::<BoundedScope>(s).is_err());
    }
    #[test]
    fn review_binds_exact_result_and_does_not_renew_authority() {
        let mut c = BoundedCommand {
            request_id: Uuid::new_v4(),
            operation: "review".into(),
            expected_revision: 1,
            scope_sha256: "a".repeat(64),
            confirmed: true,
            task_id: Some(Uuid::new_v4()),
            result_sha256: Some("b".repeat(64)),
            accept: Some(true),
            retain_memory: Some(true),
        };
        c.validate().unwrap();
        c.accept = Some(false);
        assert!(c.validate().is_err());
        c.operation = "renew".into();
        assert!(c.validate().is_err());
        c.operation = "start".into();
        assert!(c.validate().is_err());
    }
}
