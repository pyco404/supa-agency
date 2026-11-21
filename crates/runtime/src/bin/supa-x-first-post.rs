//! Explicit one-post local command. Default/prepare performs no network or mutation.
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    io::{self, IsTerminal, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use supa_domain::{Error, Result};
use supa_persistence::Store;
use supa_providers::x_oauth::local::authorization::AuthorizationReceipt;
use supa_providers::x_review::{
    self, MockAccount, MockDispatchGuard, MockResponse, MockXAdapter, MockXTransport, XReceipt,
};
use uuid::Uuid;
const OWNER: &str = "6fd863e1-745d-4a06-b68e-eea69e854a69";
use supa_runtime::x_pilot_draft::{HASH, TEXT, validate_draft};
fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::Denied)
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn private_read(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x20000 | 0x800)
        .open(path)
        .map_err(|_| Error::Denied)?;
    let m = file.metadata().map_err(|_| Error::Denied)?;
    if !m.is_file()
        || m.uid()
            != std::fs::metadata("/proc/self")
                .map_err(|_| Error::Denied)?
                .uid()
        || m.permissions().mode() & 0o777 != 0o600
        || m.nlink() != 1
        || m.len() > 16384
    {
        return Err(Error::Denied);
    }
    let mut raw = Vec::new();
    Read::by_ref(&mut file)
        .take(16385)
        .read_to_end(&mut raw)
        .map_err(|_| Error::Denied)?;
    if raw.len() > 16384 {
        return Err(Error::Denied);
    }
    Ok(raw)
}
fn identity(mock: bool) -> Result<(MockAccount, String)> {
    if mock {
        return Ok((
            MockAccount {
                id: "777".into(),
                username: "supa_agency".into(),
                mode: "mock".into(),
            },
            "a".repeat(64),
        ));
    }
    let raw = private_read(&root().join("artifacts/x-oauth1-private/identity-verification.json"))?;
    let r: Value = serde_json::from_slice(&raw).map_err(|_| Error::Denied)?;
    let a = &r["identity"];
    let id = a["account_id"].as_str().ok_or(Error::Denied)?;
    if r["state"] != "verified"
        || r["request_count"] != 1
        || a["username"] != "supa_agency"
        || a["auth_mode"] != "oauth1"
        || a["publishing_enabled"] != false
        || id.starts_with('0')
        || !id.bytes().all(|v| v.is_ascii_digit())
        || id.parse::<u64>().is_err()
    {
        return Err(Error::Denied);
    }
    Ok((
        MockAccount {
            id: id.into(),
            username: "supa_agency".into(),
            mode: "live".into(),
        },
        x_review::text_sha256(std::str::from_utf8(&raw).map_err(|_| Error::Denied)?),
    ))
}
struct Guard {
    store: Store,
    owner: Uuid,
    id: Uuid,
}
#[async_trait]
impl MockDispatchGuard for Guard {
    async fn check_reserved(&self, hash: &str, account: &MockAccount) -> Result<()> {
        self.check("peek", hash, account).await
    }
    async fn authorize_exact(&self, hash: &str, account: &MockAccount) -> Result<()> {
        self.check("claim", hash, account).await
    }
}
impl Guard {
    async fn check(&self, op: &str, hash: &str, account: &MockAccount) -> Result<()> {
        let r = self
            .store
            .first_post_command(
                self.owner,
                op,
                json!({"authorization_id":self.id,"text_sha256":hash,"account":account}),
            )
            .await?;
        if r != true {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
struct Wire {
    store: Store,
    owner: Uuid,
    id: Uuid,
    mode: String,
    calls: Arc<Mutex<Vec<String>>>,
}
#[async_trait]
impl MockXTransport for Wire {
    async fn exchange(
        &self,
        path: &str,
        body: Option<Value>,
        credential: &str,
    ) -> Result<MockResponse> {
        if credential != "mock-x-credential-no-real-account" {
            return Err(Error::Denied);
        }
        self.calls.lock().unwrap().push(path.into());
        if path == "/2/users/me" {
            if self.mode == "cancel" {
                self.store
                    .first_post_command(self.owner, "cancel", json!({"authorization_id":self.id}))
                    .await?;
            }
            return Ok(MockResponse{status:200,body:serde_json::to_vec(&json!({"data":{"id":if self.mode=="account" {"778"}else{"777"},"username":"supa_agency"}})).unwrap(),rate_limit_reset:None});
        }
        if path != "/2/tweets" || body.as_ref().is_none_or(|b| b != &json!({"text":TEXT})) {
            return Err(Error::Denied);
        }
        if self.mode == "disconnect" {
            return Err(Error::ProviderTransport);
        }
        Ok(MockResponse {
            status: match self.mode.as_str() {
                "credits" => 402,
                "permission" => 403,
                "rate" => 429,
                _ => 201,
            },
            body: serde_json::to_vec(&json!({"data":{"id":"456","text":TEXT}})).unwrap(),
            rate_limit_reset: None,
        })
    }
}
fn confirm(prompt: &str, exact: &str) -> Result<()> {
    eprintln!("{prompt}");
    io::stderr().flush().map_err(|_| Error::Denied)?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|_| Error::Denied)?;
    if line.trim() != exact {
        return Err(Error::Denied);
    }
    Ok(())
}
fn save_receipt(id: Uuid, approval: &Value, receipt: &XReceipt) -> Result<PathBuf> {
    let dir = root().join("artifacts/x-oauth1-private");
    let m = std::fs::symlink_metadata(&dir).map_err(|_| Error::Denied)?;
    if !m.is_dir()
        || m.file_type().is_symlink()
        || m.permissions().mode() & 0o777 != 0o700
        || m.uid()
            != std::fs::metadata("/proc/self")
                .map_err(|_| Error::Denied)?
                .uid()
    {
        return Err(Error::Denied);
    }
    let path = dir.join(format!("first-post-{id}.json"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|_| Error::Denied)?;
    let bytes=serde_json::to_vec_pretty(&json!({"approval":approval,"receipt":receipt,"autonomous_publishing_enabled":false,"actual_billed_cost_usd":null})).map_err(|_|Error::Denied)?;
    file.write_all(&bytes).map_err(|_| Error::Denied)?;
    file.sync_all().map_err(|_| Error::Denied)?;
    std::fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::Denied)?;
    Ok(path)
}
async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "post") {
        // Historical one-post command is tied to a tweet already published.
        // Never silently fall back to it while preparing a hosted pilot.
        return Err(Error::Denied);
    }
    if args.first().is_some_and(|arg| arg == "prepare-draft") {
        if args.len() != 3 {
            return Err(Error::Denied);
        }
        let (account, proof) = identity(false)?;
        let draft = validate_draft(&private_read(Path::new(&args[1]))?, &args[2], &account)?;
        println!(
            "{}",
            json!({"prepared":true,"text":draft.text,
            "text_sha256":draft.text_sha256,"account":draft.account,
            "identity_receipt_sha256":proof,"maximum_identity_lookups":1,
            "maximum_posts":1,"budget_microusd":25000,"maximum_approval_seconds":600,
            "authorization_required":true,"hosted_dispatch_available":false,
            "publishing_enabled":false,"external_requests":0,"automatic_retry":false})
        );
        return Ok(());
    }
    if args.is_empty() || args == ["prepare"] {
        println!(
            "{}",
            json!({"prepared":false,"reason":"historical_tweet_retired_exact_draft_required",
                "next":"prepare-draft <private-json-path> <independently-reviewed-sha256>",
                "publishing_enabled":false,"external_requests":0})
        );
        return Ok(());
    }
    if args == ["ledger-check"] {
        let _ = identity(false).map_err(|_| {
            eprintln!("ledger_identity_evidence_unavailable");
            Error::Denied
        })?;
        let url = std::env::var("DATABASE_URL").map_err(|_| Error::Denied)?;
        let store = Store::connect_first_post_local(&url, false)
            .await
            .map_err(|_| {
                eprintln!("ledger_local_database_connection_or_identity_failed");
                Error::Denied
            })?;
        store.verify_first_post_runtime_role().await.map_err(|_| {
            eprintln!("ledger_restricted_role_check_failed");
            Error::Denied
        })?;
        println!(
            "{}",
            json!({"database":"supa_x_first_post","database_target_verified":true,"restricted_role_verified":true,"private_identity_receipt_verified":true,"database_writes":0,"X_api_requests":0,"publishing_enabled":false,"permissions":"owner console confirmation required","credits":"owner console confirmation required"})
        );
        return Ok(());
    }
    let recover = args
        .first()
        .is_some_and(|s| s == "recover" || s == "mock-recover");
    let mock = args
        .first()
        .is_some_and(|s| s == "mock" || s == "mock-recover");
    let (id, expiry, mode) = if recover && args.len() == 2 {
        (
            args[1].parse::<Uuid>().map_err(|_| Error::Denied)?,
            0,
            "recover",
        )
    } else if mock && args.len() == 4 {
        (
            args[2].parse().map_err(|_| Error::Denied)?,
            args[3].parse::<u64>().map_err(|_| Error::Denied)?,
            args[1].as_str(),
        )
    } else {
        return Err(Error::Denied);
    };
    if id.is_nil() || (!recover && (expiry <= now()? || expiry > now()?.saturating_add(600))) {
        return Err(Error::Denied);
    }
    if mock
        && !matches!(
            mode,
            "ok" | "credits"
                | "permission"
                | "rate"
                | "disconnect"
                | "account"
                | "cancel"
                | "unauthorized"
                | "crash"
                | "recover"
        )
    {
        return Err(Error::Denied);
    }
    let session = if mock {
        None
    } else {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal()
        {
            return Err(Error::Denied);
        }
        let limits = std::fs::read_to_string("/proc/self/limits").map_err(|_| Error::Denied)?;
        if !limits
            .lines()
            .find_map(|s| s.strip_prefix("Max core file size"))
            .is_some_and(|s| s.split_whitespace().next() == Some("0"))
        {
            return Err(Error::Denied);
        }
        Some(AuthorizationReceipt::local_owner_session(
            x_review::oauth1::NAMESPACE,
        )?)
    };
    let owner = OWNER.parse().map_err(|_| Error::Denied)?;
    let (account, proof) = identity(mock)?;
    if !mock && !recover {
        eprintln!(
            "Exact proposed post: {TEXT}\nMaximum $0.025; one GET and at most one POST; no retry or autonomous activation."
        );
        confirm(
            "After checking the X console, type READ WRITE AND CREDITS CONFIRMED:",
            "READ WRITE AND CREDITS CONFIRMED",
        )?;
        confirm(
            &format!("Type POST {HASH} ONCE to confirm the separately authorized post:"),
            &format!("POST {HASH} ONCE"),
        )?;
    }
    let url = std::env::var(if mock {
        "SUPA_X_FIRST_POST_TEST_URL"
    } else {
        "DATABASE_URL"
    })
    .map_err(|_| Error::Denied)?;
    let store = Store::connect_first_post_local(&url, mock).await?;
    store.verify_first_post_runtime_role().await?;
    if recover {
        let result = store
            .first_post_command(owner, "recover", json!({"authorization_id":id}))
            .await?;
        println!(
            "{}",
            json!({"state":result["state"],"recovery_only":true,"external_requests":0})
        );
        return Ok(());
    }
    let expiry_time =
        chrono::DateTime::from_timestamp(expiry.try_into().map_err(|_| Error::Denied)?, 0)
            .ok_or(Error::Denied)?
            .to_rfc3339();
    let approval = json!({"authorization_id":id,"account":account,"text":TEXT,"text_sha256":HASH,"expires_at":expiry_time,"budget_microusd":25000,"confirmed":mode!="unauthorized","read_write_confirmed":true,"credits_confirmed":true,"identity_receipt_sha256":proof});
    if let Some(s) = &session {
        s.check()?;
    }
    let reserve = store
        .first_post_command(owner, "reserve", approval.clone())
        .await?;
    if reserve["fresh"] != true {
        println!(
            "{}",
            json!({"state":"already_reserved_no_retry","external_requests":0})
        );
        return Ok(());
    }
    if mock && mode == "crash" {
        println!(
            "{}",
            json!({"state":"reserved_interruption_fixture","external_requests":0})
        );
        return Ok(());
    }
    let guard = Guard {
        store: store.clone(),
        owner,
        id,
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let result = if let Some(s) = &session {
        x_review::oauth1::dispatch_reserved_once(
            s,
            &x_review::oauth1::ReservedPost {
                account: account.clone(),
                text: TEXT,
                text_sha256: HASH,
                expires_unix: expiry,
                budget_microusd: 25000,
            },
            &guard,
        )
        .await
    } else {
        guard.check_reserved(HASH, &account).await?;
        let adapter = MockXAdapter::new(
            Wire {
                store: store.clone(),
                owner,
                id,
                mode: mode.into(),
                calls: calls.clone(),
            },
            "mock-x-credential-no-real-account".into(),
            account,
            expiry,
        )?;
        Ok(adapter
            .execute_reserved_once(TEXT, HASH, now()?, &guard)
            .await)
    };
    let receipt = result.unwrap_or_else(|_| XReceipt {
        mode: if mock { "mock" } else { "live" }.into(),
        state: "failed".into(),
        category: "pre_dispatch_authorization_or_custody_unavailable".into(),
        post_id: None,
        http_status: None,
        rate_limit_reset: None,
    });
    // A crash or receipt/settlement failure keeps the durable reservation fenced.
    let local = if mock {
        None
    } else {
        Some(save_receipt(id, &approval, &receipt)?)
    };
    let settled = store
        .first_post_command(
            owner,
            "settle",
            json!({"authorization_id":id,"receipt":receipt}),
        )
        .await?;
    println!(
        "{}",
        json!({"mode":if mock {"mock"}else{"live"},"state":settled["state"],"http_status":settled["http_status"],"post_id":settled["post_id"],"receipt_path":local,"mock_calls":if mock {Some(calls.lock().unwrap().len())}else{None},"autonomous_publishing_enabled":false,"automatic_retry":false})
    );
    Ok(())
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "STOPPED: first-post operation unconfirmed; no automatic retry. Preserve the durable reservation and review/recover it before any further attempt."
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod exact_draft_tests {
    use super::*;
    fn account() -> MockAccount {
        MockAccount {
            id: "123".into(),
            username: "supa_agency".into(),
            mode: "live".into(),
        }
    }
    fn raw(text: &str) -> (Value, String) {
        let hash = x_review::text_sha256(text);
        (
            json!({"version":1,"text":text,"text_sha256":hash,"account":account()}),
            hash,
        )
    }
    fn validate(value: &Value, hash: &str) -> bool {
        validate_draft(&serde_json::to_vec(value).unwrap(), hash, &account()).is_ok()
    }
    #[test]
    fn exact_fresh_draft_preserves_bytes_without_authorizing() {
        let (v, h) = raw("Synthetic fixture completed three bounded cycles and stopped.");
        let d = validate_draft(&serde_json::to_vec(&v).unwrap(), &h, &account()).unwrap();
        assert_eq!(d.text, v["text"]);
        assert_eq!(d.text_sha256, h);
    }
    #[test]
    fn rejects_historical_tweet_and_normalized_variants() {
        for text in [
            TEXT.to_owned(),
            TEXT.to_uppercase(),
            TEXT.replace(" ", "  "),
        ] {
            let (v, h) = raw(&text);
            assert!(!validate(&v, &h));
        }
    }
    #[test]
    fn rejects_changed_content_or_independent_hash() {
        let (mut v, h) = raw("Synthetic fixture is complete.");
        assert!(!validate(&v, &"0".repeat(64)));
        v["text"] = json!("Synthetic fixture is different.");
        assert!(!validate(&v, &h));
    }
    #[test]
    fn rejects_account_or_mode_substitution() {
        for (field, bad) in [("id", "456"), ("username", "other"), ("mode", "mock")] {
            let (mut v, h) = raw("Synthetic fixture is complete.");
            v["account"][field] = json!(bad);
            assert!(!validate(&v, &h));
        }
    }
    #[test]
    fn rejects_missing_unknown_and_malformed_fields() {
        let (mut v, h) = raw("Synthetic fixture is complete.");
        v["approved"] = json!(true);
        assert!(!validate(&v, &h));
        v.as_object_mut().unwrap().remove("approved");
        v.as_object_mut().unwrap().remove("text_sha256");
        assert!(!validate(&v, &h));
        assert!(validate_draft(b"{", &h, &account()).is_err());
    }
    #[test]
    fn retains_text_only_and_security_policy() {
        for text in [
            "",
            " New draft ",
            "Visit example.com",
            "Hello @other",
            "https://example.com",
        ] {
            let (v, h) = raw(text);
            assert!(!validate(&v, &h));
        }
    }
    #[test]
    fn rejects_noncanonical_hash_and_schema_version() {
        let (mut v, h) = raw("Synthetic fixture is complete.");
        assert!(!validate(&v, &h.to_uppercase()));
        v["version"] = json!(2);
        assert!(!validate(&v, &h));
    }
}
