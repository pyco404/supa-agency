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
const TEXT: &str = "Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip.";
const HASH: &str = "e8bc2bee94696da203aad809d8a80aeb05a4677960ef34d9b0b68b82d47f2dd7";
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
    if args.is_empty() || args == ["prepare"] {
        let (_, proof) = identity(false)?;
        println!(
            "{}",
            json!({"prepared":true,"text":TEXT,"text_sha256":HASH,"identity_receipt_sha256":proof,"numeric_account_id":"verified privately","budget_microusd":25000,"ledger_status":"not connected; separately approved local ledger setup required","permissions":"owner console confirmation required","credits":"owner console confirmation required","publishing_enabled":false,"external_requests":0})
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
    } else if args.len() == 4
        && args[0] == "post"
        && args[3] == "--authorize-one-get-and-one-post-budget-025"
    {
        (
            args[1].parse().map_err(|_| Error::Denied)?,
            args[2].parse::<u64>().map_err(|_| Error::Denied)?,
            "live",
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
