//! Trusted owner identity, released metadata and exact-input review. No X credentials.
use crate::*;
use serde::{Deserialize, Serialize};
use supa_domain::social_review;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocialReviewCommand {
    pub request_id: Uuid,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<u32>,
}
impl SocialReviewCommand {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || !matches!(
                self.operation.as_str(),
                "release"
                    | "pause"
                    | "create"
                    | "edit"
                    | "revise"
                    | "approve"
                    | "reject"
                    | "request_revision"
            )
            || self.expected_revision.is_some_and(|v| v >= 64)
            || self.variant.is_some_and(|v| v > 3)
        {
            return Err(Error::Invalid);
        }
        if let Some(expires) = &self.expires_at {
            chrono::DateTime::parse_from_rfc3339(expires).map_err(|_| Error::Invalid)?;
        }
        if self.text_sha256.as_ref().is_some_and(|s| {
            s.len() != 64
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            return Err(Error::Invalid);
        }
        if let Some(text) = &self.text {
            social_review::safe_post(text)?;
        }
        Ok(())
    }
}
impl Store {
    async fn social_sql(&self, actor: Uuid, op: &str, body: Value) -> Result<Value> {
        if self.operational_context().is_some() || actor.is_nil() {
            return Err(Error::Denied);
        }
        sqlx::query_scalar("SELECT social_review.command($1,$2,$3)")
            .bind(actor)
            .bind(op)
            .bind(body)
            .fetch_one(self.pool())
            .await
            .map_err(
                |e| match e.as_database_error().and_then(|d| d.code()).as_deref() {
                    Some("42501") => Error::Denied,
                    Some("40001") => Error::Stale,
                    Some("54000") => Error::Denied,
                    _ => db(e),
                },
            )
    }
    pub async fn social_review_read(&self, actor: Uuid) -> Result<Value> {
        let mut value = self.social_sql(actor, "read", json!({})).await?;
        // Metadata-to-public phrases reuse the established verifier; no private prose.
        if let Some(sources) = value["context"]["sources"].as_array_mut() {
            for source in sources {
                let e: Event =
                    serde_json::from_value(source["event"].clone()).map_err(|_| Error::Invalid)?;
                source["public_fact"] = json!(social_fact(&e).ok_or(Error::Invalid)?.1);
            }
        }
        Ok(value)
    }
    pub async fn social_review_command(
        &self,
        actor: Uuid,
        command: &SocialReviewCommand,
    ) -> Result<Value> {
        command.validate()?;
        let client = serde_json::to_value(command).map_err(|_| Error::Invalid)?;
        let old = self
            .social_sql(actor, "replay", json!({"client":client}))
            .await?;
        if !old.is_null() {
            return Ok(old);
        }
        let read = self.social_review_read(actor).await?;
        let context = &read["context"];
        let proposal = if matches!(command.operation.as_str(), "create" | "edit" | "revise") {
            match social_review::proposal(
                context,
                &command.operation,
                command.draft_id,
                command.text.as_deref(),
                command.variant.unwrap_or(0),
            ) {
                Ok(proposal) => proposal,
                Err(error) => {
                    // Another identical request may have committed between replay
                    // lookup and this read. Re-read its receipt; never regenerate.
                    let replay = self
                        .social_sql(actor, "replay", json!({"client":client}))
                        .await?;
                    if !replay.is_null() {
                        return Ok(replay);
                    }
                    return Err(error);
                }
            }
        } else {
            if matches!(command.operation.as_str(), "approve") {
                let row = context["drafts"]
                    .as_array()
                    .ok_or(Error::Invalid)?
                    .iter()
                    .find(|d| d["id"] == json!(command.draft_id))
                    .ok_or(Error::NotFound)?;
                let d: SocialDecision =
                    serde_json::from_value(row["decision"].clone()).map_err(|_| Error::Invalid)?;
                social_review::validate(&d, &social_review::context(context, command.draft_id)?)?;
            }
            Value::Null
        };
        let result=self.social_sql(actor,"apply",json!({"client":client,"proposal":proposal,"context_sha256":read["context_sha256"]})).await?;
        // Return the same allowlisted view as GET; replay receipts stay immutable.
        Ok(result)
    }
}

impl Store {
    /// Local trusted backend only. No HTTP actor or automatic schema/policy installation.
    pub async fn first_post_command(&self, actor: Uuid, op: &str, body: Value) -> Result<Value> {
        if self.operational_context().is_some() || actor.is_nil() {
            return Err(Error::Denied);
        }
        sqlx::query_scalar("SELECT social_review.first_post($1,$2,$3)")
            .bind(actor)
            .bind(op)
            .bind(body)
            .fetch_one(self.pool())
            .await
            .map_err(|_| Error::Denied)
    }
    pub async fn connect_first_post_local(url: &str, testing: bool) -> Result<Self> {
        use sqlx::ConnectOptions;
        let options: sqlx::postgres::PgConnectOptions = url.parse().map_err(|_| Error::Denied)?;
        if options.get_database()
            != Some(if testing {
                "supa_autonomy_test"
            } else {
                "supa_x_first_post"
            })
            || !matches!(options.get_host(), "127.0.0.1" | "localhost" | "::1")
        {
            return Err(Error::Denied);
        }
        if let Some(socket) = options.get_socket() {
            let parent = socket.parent().ok_or(Error::Denied)?;
            if !testing
                || parent.parent() != Some(std::path::Path::new("/tmp"))
                || socket.file_name().and_then(|s| s.to_str()) != Some("socket")
                || !parent
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| {
                        s.strip_prefix("supa-autonomy-test-").is_some_and(|v| {
                            v.len() == 32 && v.bytes().all(|c| c.is_ascii_hexdigit())
                        })
                    })
            {
                return Err(Error::Denied);
            }
        }
        let store = Self {
            operational: None,
            status_pool: None,
            pool: PgPoolOptions::new()
                .max_connections(2)
                .acquire_timeout(std::time::Duration::from_secs(5))
                .connect_with(options.disable_statement_logging())
                .await
                .map_err(|_| Error::Denied)?,
        };
        store.verify_first_post_local_database(testing).await?;
        Ok(store)
    }
    pub async fn verify_first_post_runtime_role(&self) -> Result<()> {
        let restricted:bool=sqlx::query_scalar("SELECT NOT (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls) AND NOT has_table_privilege(current_user,'public.agents','INSERT,UPDATE,DELETE,TRUNCATE') AND NOT has_table_privilege(current_user,'social_review.drafts','INSERT,UPDATE,DELETE,TRUNCATE') AND NOT has_table_privilege(current_user,'public.agent_access','INSERT,UPDATE,DELETE,TRUNCATE') AND has_function_privilege(current_user,'social_review.first_post(uuid,text,jsonb)','EXECUTE') FROM pg_roles WHERE rolname=current_user").fetch_one(self.pool()).await.map_err(|_|Error::Denied)?;
        if !restricted {
            return Err(Error::Denied);
        }
        Ok(())
    }
    /// Fail closed outside a dedicated local ledger or the existing disposable test fixture.
    pub async fn verify_first_post_local_database(&self, testing: bool) -> Result<()> {
        let (name, version, addr): (String, String, Option<String>) = sqlx::query_as(
            "SELECT current_database(),current_setting('server_version'),inet_server_addr()::text",
        )
        .fetch_one(self.pool())
        .await
        .map_err(|_| Error::Denied)?;
        let expected = if testing {
            "supa_autonomy_test"
        } else {
            "supa_x_first_post"
        };
        if name != expected
            || !version.starts_with("18.")
            || addr
                .as_deref()
                .is_some_and(|v| v != "127.0.0.1" && v != "::1")
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
