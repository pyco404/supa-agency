//! Bounded social authorization; live transport additionally requires external activation. Actor must come from server authentication.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomousSocialGrant {
    pub grant_id: Uuid,
    pub account: Value,
    pub confirmed: bool,
    pub scope: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub max_posts: u8,
    pub interval_seconds: u32,
    pub budget_microusd: i64,
}
impl AutonomousSocialGrant {
    pub fn validate(&self) -> Result<()> {
        if self.grant_id.is_nil()
            || !self.confirmed
            || self.scope != "original_text_released_metadata_only"
            || self.account["mode"] != "mock"
            || self.account["username"] != "supa_agency"
            || !(1..=6).contains(&self.max_posts)
            || !(14400..=86400).contains(&self.interval_seconds)
            || !(25000..=10000000).contains(&self.budget_microusd)
            || self.expires_at <= Utc::now()
            || self.expires_at > Utc::now() + chrono::Duration::days(7)
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
impl Store {
    pub async fn autonomous_social_authorize_mock(
        &self,
        actor: Uuid,
        grant: &AutonomousSocialGrant,
    ) -> Result<Value> {
        grant.validate()?;
        self.autonomous_social_command_mock(
            actor,
            "authorize",
            serde_json::to_value(grant).map_err(|_| Error::Invalid)?,
        )
        .await
    }
    /// Mock entry point cannot be repurposed with a live policy.

    pub async fn autonomous_social_command_mock(
        &self,
        actor: Uuid,
        op: &str,
        body: Value,
    ) -> Result<Value> {
        self.autonomous_social_command_mode(actor, op, body, "mock")
            .await
    }
    /// Trusted backend only. No HTTP endpoint or browser-supplied actor is installed.
    /// The runtime verifies root activation before dispatch; this method never loads tokens.
    pub async fn autonomous_social_command_live(
        &self,
        actor: Uuid,
        op: &str,
        body: Value,
    ) -> Result<Value> {
        self.autonomous_social_command_mode(actor, op, body, "live")
            .await
    }
    async fn autonomous_social_command_mode(
        &self,
        actor: Uuid,
        op: &str,
        body: Value,
        mode: &str,
    ) -> Result<Value> {
        if actor.is_nil() || self.operational_context().is_some() {
            return Err(Error::Denied);
        }
        let configured: Option<String> =
            sqlx::query_scalar("SELECT social_autonomy.policy_mode($1)")
                .bind(actor)
                .fetch_one(self.pool())
                .await
                .map_err(db)?;
        if configured.as_deref() != Some(mode) {
            return Err(Error::Denied);
        }
        if op == "reserve" {
            let read: Value = sqlx::query_scalar("SELECT social_autonomy.command($1,'read',$2)")
                .bind(actor)
                .bind(json!({"grant_id":body["grant_id"]}))
                .fetch_one(self.pool())
                .await
                .map_err(db)?;
            let mut expected = supa_domain::social_review::autonomous_proposal(&read["context"])?;
            expected["text_sha256"] = body["proposal"]["text_sha256"].clone();
            if expected != body["proposal"] {
                return Err(Error::Denied);
            }
        }
        sqlx::query_scalar("SELECT social_autonomy.command($1,$2,$3)")
            .bind(actor)
            .bind(op)
            .bind(body)
            .fetch_one(self.pool())
            .await
            .map_err(
                |e| match e.as_database_error().and_then(|d| d.code()).as_deref() {
                    Some("42501") => Error::Denied,
                    Some("40001") => Error::Stale,
                    _ => db(e),
                },
            )
    }
}
