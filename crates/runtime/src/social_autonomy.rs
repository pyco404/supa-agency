//! Bounded scheduler engine, explicitly mock-only. No background task or credential lookup.
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
/// No real posting adapter, worker launch or activation switch is available.
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
