//! Owner-scoped task persistence; no writes to frozen agents or financial tables.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisedInput {
    pub task: String,
    pub desired_outcome: String,
    pub material: String,
    pub synthetic_only: bool,
    pub retain_memory: bool,
}
impl SupervisedInput {
    pub fn validate(&self) -> Result<()> {
        if !self.synthetic_only || self.material.len() > 4000 {
            return Err(Error::Invalid);
        }
        for (s, n) in [
            (&self.task, 600),
            (&self.desired_outcome, 120),
            (&self.material, 4000),
        ] {
            clean_text(s, n)?;
            if s.trim() != s
                || s.chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
            {
                return Err(Error::Invalid);
            }
            validate_work_source(s)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisedCreate {
    pub request_id: Uuid,
    pub input: SupervisedInput,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisedCommand {
    pub request_id: Uuid,
    pub operation: String,
    pub expected_revision: u32,
    pub input_sha256: String,
    pub confirmed: bool,
}
impl SupervisedCommand {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.expected_revision > 33
            || self.input_sha256.len() != 64
            || !self
                .input_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || !["approve", "start", "advance", "pause", "resume", "stop"]
                .contains(&self.operation.as_str())
            || (matches!(self.operation.as_str(), "approve" | "start" | "resume")
                && !self.confirmed)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
impl Store {
    pub async fn verify_supervised_schema(&self) -> Result<()> {
        let valid:bool=sqlx::query_scalar("SELECT has_schema_privilege(current_user,'agent_supervised','USAGE') AND has_function_privilege(current_user,'agent_supervised.command(uuid,text,jsonb)','EXECUTE') AND NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=current_user AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolbypassrls)) AND NOT has_schema_privilege(current_user,'agent_supervised','CREATE') AND NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='agent_supervised' AND c.relkind='r' AND has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE'))")
            .fetch_one(&self.pool).await.map_err(db)?;
        if !valid {
            return Err(Error::Denied);
        }
        Ok(())
    }

    pub async fn supervised(&self, actor: Uuid, operation: &str, body: Value) -> Result<Value> {
        if self.operational.is_some() || actor.is_nil() {
            return Err(Error::Denied);
        }
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query(
            "SELECT set_config('statement_timeout','2s',true),set_config('lock_timeout','1s',true)",
        )
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        let value = sqlx::query_scalar("SELECT agent_supervised.command($1,$2,$3)")
            .bind(actor)
            .bind(operation)
            .bind(body)
            .fetch_one(&mut *tx)
            .await
            .map_err(
                |e| match e.as_database_error().and_then(|e| e.code()).as_deref() {
                    Some("42501") => Error::Denied,
                    Some("40001" | "23505") => Error::Stale,
                    Some("22023" | "22P02" | "23514") => Error::Invalid,
                    Some("54000") => Error::ResourceBudget,
                    Some("57014" | "55P03") => Error::Timeout,
                    _ => db(e),
                },
            )?;
        tx.commit().await.map_err(db)?;
        Ok(value)
    }
    pub async fn supervised_create(
        &self,
        actor: Uuid,
        agent: Uuid,
        request: &SupervisedCreate,
    ) -> Result<Value> {
        request.input.validate()?;
        if request.request_id.is_nil() {
            return Err(Error::Invalid);
        }
        self.supervised(
            actor,
            "create",
            json!({"agent_id":agent,"request_id":request.request_id,"input":request.input}),
        )
        .await
    }
}
