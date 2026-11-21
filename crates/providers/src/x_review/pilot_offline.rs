//! Compile-time-only, non-network transport for the NON-DEPLOYABLE pilot harness.
//! No OAuth parsing, sockets, reqwest, configurable host or production selector.
use super::{PilotAuthorization, at, protected, read};
use crate::x_review::{MockDispatchGuard, MockResponse, MockXAdapter, MockXTransport, XReceipt};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::atomic::{AtomicU8, Ordering},
};
use supa_domain::{Error, Result};
use zeroize::Zeroizing;
const CASE: &str = "/etc/supa-agent/x-pilot/offline-case.json";
const JOURNAL: &str = "/etc/supa-agent/x-worker/offline-wire.jsonl";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    scenario: String,
}
fn isolated() -> Result<()> {
    let interfaces = fs::read_dir("/sys/class/net").map_err(|_| Error::Denied)?;
    for entry in interfaces {
        if entry.map_err(|_| Error::Denied)?.file_name() != "lo" {
            return Err(Error::Denied);
        }
    }
    if Path::new("/run/credentials/supa-x-autonomy.service/oauth1.json").exists() {
        return Err(Error::Denied);
    }
    Ok(())
}
struct Wire<'a> {
    auth: &'a PilotAuthorization,
    scenario: String,
    calls: AtomicU8,
}
#[async_trait]
impl MockXTransport for Wire<'_> {
    async fn exchange(
        &self,
        path: &str,
        body: Option<Value>,
        credential: &str,
    ) -> Result<MockResponse> {
        isolated()?;
        self.auth.check()?;
        if !credential.is_empty() {
            return Err(Error::Denied);
        }
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if !matches!((call, path), (0, "/2/users/me") | (1, "/2/tweets")) {
            return Err(Error::Denied);
        }
        if path == "/2/tweets" && body != Some(json!({"text":self.auth_text_check(&body)?})) {
            return Err(Error::Denied);
        }
        // Record counts and route labels only; never draft, receipt or credentials.
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(JOURNAL)
            .map_err(|_| Error::Denied)?;
        writeln!(
            file,
            "{}",
            json!({"route":path,"call":call+1,"offline":true})
        )
        .map_err(|_| Error::Denied)?;
        file.sync_all().map_err(|_| Error::Denied)?;
        if path == "/2/users/me" {
            if self.scenario == "stop-after-identity" {
                // Root fixture prepares a stop marker after observing the first call.
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            return Ok(MockResponse { status:200, body:serde_json::to_vec(&json!({"data":{"id":if self.scenario=="wrong-identity"{"2"}else{"1"},"username":"supa_agency"}})).map_err(|_| Error::Denied)?, rate_limit_reset:None });
        }
        if self.scenario == "uncertain" {
            return Err(Error::ProviderTransport);
        }
        if self.scenario == "wait-post" {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
        Ok(MockResponse {
            status: 201,
            body: serde_json::to_vec(
                &json!({"data":{"id":"123456","text":body.ok_or(Error::Denied)?["text"]}}),
            )
            .map_err(|_| Error::Denied)?,
            rate_limit_reset: None,
        })
    }
}
impl Wire<'_> {
    fn auth_text_check(&self, body: &Option<Value>) -> Result<String> {
        let text = body
            .as_ref()
            .and_then(|v| v["text"].as_str())
            .ok_or(Error::Denied)?;
        if crate::x_review::text_sha256(text) != self.auth.receipt().text_sha256 {
            return Err(Error::Denied);
        }
        Ok(text.into())
    }
}
pub(super) async fn dispatch(
    auth: &PilotAuthorization,
    text: &str,
    guard: &dyn MockDispatchGuard,
) -> Result<XReceipt> {
    isolated()?;
    auth.check()?;
    let case: Case = serde_json::from_slice(&read(Path::new(CASE))?).map_err(|_| Error::Denied)?;
    if !matches!(
        case.scenario.as_str(),
        "success" | "wrong-identity" | "uncertain" | "stop-after-identity" | "wait-post"
    ) {
        return Err(Error::Denied);
    }
    let r = auth.receipt();
    // Synthetic numeric identity is mandatory; a real account cannot use the harness.
    if r.account.id != "1" || crate::x_review::text_sha256(text) != r.text_sha256 {
        return Err(Error::Denied);
    }
    guard.check_reserved(&r.text_sha256, &r.account).await?;
    protected(Path::new(CASE), false)?;
    let wire = Wire {
        auth,
        scenario: case.scenario,
        calls: AtomicU8::new(0),
    };
    Ok(MockXAdapter::prepared_live(
        wire,
        Zeroizing::new(String::new()),
        r.account.clone(),
        r.expires_unix,
    )?
    .execute_reserved_once(text, &r.text_sha256, at()?, guard)
    .await)
}
