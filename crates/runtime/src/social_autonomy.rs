//! Explicit bounded scheduler invocations. No automatic production worker startup.
use async_trait::async_trait;
use serde_json::{Value, json};
use supa_domain::{Error, Result, social_review};
use supa_persistence::Store;
use supa_providers::x_review::{MockAccount, MockDispatchGuard, MockXAdapter, MockXTransport};
use uuid::Uuid;

struct Guard {
    store: Store,
    owner: Uuid,
    grant: Uuid,
    attempt: Uuid,
}
#[async_trait]
impl MockDispatchGuard for Guard {
    async fn authorize_exact(&self, hash: &str, account: &MockAccount) -> Result<()> {
        let answer = self
            .store
            .autonomous_social_command_mock(
                self.owner,
                "guard",
                json!({
            "grant_id":self.grant,"attempt_id":self.attempt,"text_sha256":hash,"account":account}),
            )
            .await?;
        if answer != true {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
/// Autonomous factual wording is restricted to the existing vetted generator.
/// A semantic blacklist alone is insufficient to authorize arbitrary model prose.
pub fn draft_released(context: &Value) -> Result<Value> {
    let mut proposal = social_review::autonomous_proposal(context)?;
    proposal["text_sha256"] = json!(supa_providers::x_review::text_sha256(
        proposal["text"].as_str().ok_or(Error::Invalid)?
    ));
    Ok(proposal)
}
/// One scheduler observation. Repeated polls have no effect until the durable due time.
/// Owner authorization is granted once; this path never calls per-draft approve.
/// Production connection receipts cannot be converted into autonomous grants.
pub async fn tick_mock<T: MockXTransport>(
    store: &Store,
    owner: Uuid,
    grant: Uuid,
    request: Uuid,
    transport: T,
) -> Result<Value> {
    if request.is_nil() {
        return Err(Error::Denied);
    }
    let body = json!({"grant_id":grant});
    let read = store
        .autonomous_social_command_mock(owner, "read", body.clone())
        .await?;
    if read["due"] != true {
        return Ok(json!({"mode":"mock","state":"not_due"}));
    }
    let proposal = match draft_released(&read["context"]) {
        Ok(p) => p,
        Err(_) => {
            store
                .autonomous_social_command_mock(owner, "abstain", body)
                .await?;
            return Ok(json!({"mode":"mock","state":"abstained","model_requests":0}));
        }
    };
    let reserved = store
        .autonomous_social_command_mock(
            owner,
            "reserve",
            json!({"grant_id":grant,
        "attempt_id":request,"context_sha256":read["context_sha256"],"proposal":proposal}),
        )
        .await?;
    if reserved["fresh"] != true {
        return Ok(json!({"mode":"mock","state":"already_reserved_no_retry"}));
    }
    let account: MockAccount =
        serde_json::from_value(read["grant"]["account"].clone()).map_err(|_| Error::Invalid)?;
    let expires = read["grant"]["expires_at"].as_str().ok_or(Error::Invalid)?;
    let expires = chrono::DateTime::parse_from_rfc3339(expires)
        .map_err(|_| Error::Invalid)?
        .timestamp();
    let adapter = MockXAdapter::new(
        transport,
        "mock-x-credential-no-real-account".into(),
        account,
        expires.try_into().map_err(|_| Error::Invalid)?,
    )?;
    let guard = Guard {
        store: store.clone(),
        owner,
        grant,
        attempt: request,
    };
    let receipt = adapter
        .execute_reserved_once(
            proposal["text"].as_str().ok_or(Error::Invalid)?,
            proposal["text_sha256"].as_str().ok_or(Error::Invalid)?,
            chrono::Utc::now().timestamp() as u64,
            &guard,
        )
        .await;
    // An interrupted finish leaves durable intent. Recovery records uncertainty,
    // pauses the grant and never sends the same attempt again.
    store
        .autonomous_social_command_mock(
            owner,
            "finish",
            json!({"grant_id":grant,"attempt_id":request,"receipt":receipt}),
        )
        .await
}
/// Optional local mock runner. Explicit invocation only, at most one week of minute
/// observations. Durable four-hour slots control dispatch, not poll frequency.
/// A lost control channel stops observation. No production worker is connected.
pub async fn run_mock_scheduler<T: MockXTransport, F: Fn() -> T>(
    store: &Store,
    owner: Uuid,
    grant: Uuid,
    factory: F,
    mut cancel: tokio::sync::watch::Receiver<bool>,
    max_polls: u32,
) -> Result<u32> {
    if max_polls == 0 || max_polls > 10080 {
        return Err(Error::Denied);
    }
    if *cancel.borrow() || cancel.has_changed().is_err() {
        return Ok(0);
    }
    store
        .autonomous_social_command_mock(owner, "recover", json!({"grant_id":grant}))
        .await?;
    for poll in 0..max_polls {
        let read = store
            .autonomous_social_command_mock(owner, "read", json!({"grant_id":grant}))
            .await?;
        let expires = read["grant"]["expires_at"].as_str().ok_or(Error::Invalid)?;
        if matches!(read["grant"]["state"].as_str(), Some("stopped" | "revoked"))
            || chrono::DateTime::parse_from_rfc3339(expires)
                .map_err(|_| Error::Invalid)?
                .timestamp()
                <= chrono::Utc::now().timestamp()
        {
            return Ok(poll);
        }
        tokio::select! {
            biased;
            _=cancel.changed()=>{
                store.autonomous_social_command_mock(owner,"pause",json!({"grant_id":grant})).await?;
                store.autonomous_social_command_mock(owner,"recover",json!({"grant_id":grant})).await?;
                return Ok(poll);
            },
            outcome=tick_mock(store,owner,grant,Uuid::new_v4(),factory())=>{outcome?;}
        }
        if poll + 1 < max_polls {
            tokio::select! {
                _=cancel.changed()=>{
                    store.autonomous_social_command_mock(owner,"pause",json!({"grant_id":grant})).await?;
                    return Ok(poll+1);
                },
                _=tokio::time::sleep(std::time::Duration::from_secs(60))=>{}
            }
        }
    }
    Ok(max_polls)
}
/// Protected control surface for trusted authenticated-owner callers. Expired or
/// revoked activation cannot prevent emergency pause/stop/revoke or recovery.
/// Resume requires the exact root-approved grant and current connection authority.
pub async fn live_control(
    store: &Store,
    owner: Uuid,
    grant: Uuid,
    operation: &str,
    authorization: Option<&supa_providers::x_review::live::LiveAuthorization>,
) -> Result<Value> {
    if owner.is_nil()
        || grant.is_nil()
        || !matches!(
            operation,
            "read" | "pause" | "resume" | "stop" | "revoke" | "recover"
        )
    {
        return Err(Error::Denied);
    }
    if operation == "resume" {
        let auth = authorization.ok_or(Error::Denied)?;
        verify_live_grant(store, owner, grant, auth).await?;
        supa_providers::x_review::live::verify_connection(auth)?;
    }
    store
        .autonomous_social_command_live(owner, operation, json!({"grant_id":grant}))
        .await
}
async fn verify_live_grant(
    store: &Store,
    owner: Uuid,
    grant: Uuid,
    auth: &supa_providers::x_review::live::LiveAuthorization,
) -> Result<Value> {
    auth.check()?;
    if owner != auth.owner() || auth.grant()["grant_id"] != grant.to_string() {
        return Err(Error::Denied);
    }
    let read = store
        .autonomous_social_command_live(owner, "read", json!({"grant_id":grant}))
        .await?;
    if read["mode"] != "live" || read["grant"]["grant_body"] != *auth.grant() {
        return Err(Error::Denied);
    }
    Ok(read)
}
/// Root approval is necessary even to install this bounded, initially paused grant.
/// Policy enrollment and SQL grants remain separate privileged setup actions.
pub async fn authorize_live(
    store: &Store,
    auth: &supa_providers::x_review::live::LiveAuthorization,
) -> Result<Value> {
    auth.check()?;
    store
        .autonomous_social_command_live(auth.owner(), "authorize", auth.grant().clone())
        .await
}
struct LiveGuard<'a> {
    store: &'a Store,
    auth: &'a supa_providers::x_review::live::LiveAuthorization,
    grant: Uuid,
    attempt: Uuid,
}
#[async_trait]
impl MockDispatchGuard for LiveGuard<'_> {
    async fn check_reserved(&self, hash: &str, account: &MockAccount) -> Result<()> {
        let read = verify_live_grant(self.store, self.auth.owner(), self.grant, self.auth).await?;
        if read["context"]["paused"] != false
            || read["grant"]["state"] != "active"
            || read["grant"]["account"]
                != serde_json::to_value(account).map_err(|_| Error::Invalid)?
            || !read["attempts"].as_array().is_some_and(|rows| {
                rows.iter().any(|r| {
                    r["id"] == self.attempt.to_string()
                        && r["state"] == "reserved"
                        && r["text_sha256"] == hash
                })
            })
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
    async fn authorize_exact(&self, hash: &str, account: &MockAccount) -> Result<()> {
        verify_live_grant(self.store, self.auth.owner(), self.grant, self.auth).await?;
        let answer=self.store.autonomous_social_command_live(self.auth.owner(),"guard",json!({
            "grant_id":self.grant,"attempt_id":self.attempt,"text_sha256":hash,"account":account
        })).await?;
        if answer != true {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
/// Same evidence generator, durable reservation and account-wide fence as mocks.
/// Never starts on application startup; a caller must supply live activation.
pub async fn tick_live(
    store: &Store,
    auth: &supa_providers::x_review::live::LiveAuthorization,
    request: Uuid,
) -> Result<Value> {
    if request.is_nil() {
        return Err(Error::Denied);
    }
    let grant: Uuid = auth.grant()["grant_id"]
        .as_str()
        .ok_or(Error::Denied)?
        .parse()
        .map_err(|_| Error::Denied)?;
    let owner = auth.owner();
    let read = verify_live_grant(store, owner, grant, auth).await?;
    if read["due"] != true {
        return Ok(json!({"mode":"live","state":"not_due"}));
    }
    // Expired/missing custody cannot consume a new posting slot or budget.
    if supa_providers::x_review::live::verify_connection(auth).is_err() {
        live_control(store, owner, grant, "pause", None).await?;
        return Err(Error::Denied);
    }
    let proposal = match draft_released(&read["context"]) {
        Ok(p) => p,
        Err(_) => {
            store
                .autonomous_social_command_live(owner, "abstain", json!({"grant_id":grant}))
                .await?;
            return Ok(json!({"mode":"live","state":"abstained","model_requests":0}));
        }
    };
    let reserved=store.autonomous_social_command_live(owner,"reserve",json!({
        "grant_id":grant,"attempt_id":request,"context_sha256":read["context_sha256"],"proposal":proposal
    })).await?;
    if reserved["fresh"] != true {
        return Ok(json!({"mode":"live","state":"already_reserved_no_retry"}));
    }
    let guard = LiveGuard {
        store,
        auth,
        grant,
        attempt: request,
    };
    let receipt = match supa_providers::x_review::live::dispatch_reserved_once(
        auth,
        proposal["text"].as_str().ok_or(Error::Invalid)?,
        proposal["text_sha256"].as_str().ok_or(Error::Invalid)?,
        &guard,
    )
    .await
    {
        Ok(receipt) => receipt,
        // dispatch returns Err only before protocol execution. Once any POST could
        // have started, it returns an uncertain receipt instead, never a safe failure.
        Err(_) => supa_providers::x_review::XReceipt {
            mode: "live".into(),
            state: "failed".into(),
            category: "custody_or_activation_unavailable_reauthorize".into(),
            post_id: None,
            http_status: None,
            rate_limit_reset: None,
        },
    };
    let stop = receipt.state != "posted";
    let answer = store
        .autonomous_social_command_live(
            owner,
            "finish",
            json!({"grant_id":grant,"attempt_id":request,"receipt":receipt}),
        )
        .await?;
    if stop {
        live_control(store, owner, grant, "pause", None).await?;
    }
    Ok(answer)
}
/// Explicit bounded local runner; no background worker, scheduling enrollment or
/// refresh occurs. Cancellation never clears an uncertain account outcome.
pub async fn run_live_scheduler(
    store: &Store,
    auth: &supa_providers::x_review::live::LiveAuthorization,
    mut cancel: tokio::sync::watch::Receiver<bool>,
    max_polls: u32,
) -> Result<u32> {
    if max_polls == 0 || max_polls > 10080 {
        return Err(Error::Denied);
    }
    let grant: Uuid = auth.grant()["grant_id"]
        .as_str()
        .ok_or(Error::Denied)?
        .parse()
        .map_err(|_| Error::Denied)?;
    auth.check()?;
    live_control(store, auth.owner(), grant, "recover", None).await?;
    for poll in 0..max_polls {
        if *cancel.borrow() || cancel.has_changed().is_err() || auth.check().is_err() {
            live_control(store, auth.owner(), grant, "pause", None).await?;
            return Ok(poll);
        }
        let state = verify_live_grant(store, auth.owner(), grant, auth).await?;
        if state["grant"]["state"] == "paused" {
            // Keep the supervised service available for an explicit owner resume;
            // a paused observation performs no custody read or X request.
            if poll + 1 < max_polls {
                tokio::select! {
                    _=cancel.changed()=>{},
                    _=tokio::time::sleep(std::time::Duration::from_secs(60))=>{}
                }
            }
            continue;
        }
        if state["grant"]["state"] != "active" {
            return Ok(poll);
        }
        tokio::select! {biased;
            _=cancel.changed()=>{
                live_control(store,auth.owner(),grant,"pause",None).await?;
                live_control(store,auth.owner(),grant,"recover",None).await?;
                return Ok(poll);
            },
            answer=tick_live(store,auth,Uuid::new_v4())=>{answer?;}
        }
        if poll + 1 < max_polls {
            tokio::select! {
                _=cancel.changed()=>{},
                _=tokio::time::sleep(std::time::Duration::from_secs(60))=>{}
            }
        }
    }
    live_control(store, auth.owner(), grant, "pause", None).await?;
    Ok(max_polls)
}
/// Legacy startup entry stays disabled. Only explicit approved invocation exists.
pub fn production_scheduler() -> Result<()> {
    Err(Error::Denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absence_private_prose_financial_hype_and_live_activation_fail_closed() {
        assert!(draft_released(&json!({"sources":[],"drafts":[]})).is_err());
        assert!(
            draft_released(
                &json!({"sources":[{"released":false,"event":{"private":"hidden"}}],"drafts":[]})
            )
            .is_err()
        );
        assert!(production_scheduler().is_err());
    }
}
