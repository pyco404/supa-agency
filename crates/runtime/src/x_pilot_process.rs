//! Explicit manual hosted one-shot. No scheduler, enrollment, grant or retry.
use async_trait::async_trait;
use serde_json::json;
use std::{
    io::Read,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use supa_domain::{Error, Result};
use supa_persistence::Store;
use supa_providers::x_review::{
    MockAccount, MockDispatchGuard, XReceipt, pilot::PilotAuthorization,
};
use supa_runtime::x_pilot_draft::validate_draft;
use uuid::Uuid;
struct Guard<'a> {
    store: Store,
    auth: &'a PilotAuthorization,
    stop: Arc<AtomicBool>,
}
impl Guard<'_> {
    async fn check(&self, op: &str, hash: &str, account: &MockAccount) -> Result<()> {
        self.auth.check()?;
        let r = self.auth.receipt();
        if self.stop.load(Ordering::SeqCst)
            || hash != r.text_sha256
            || account.id != r.account.id
            || account.username != r.account.username
            || account.mode != r.account.mode
        {
            return Err(Error::Denied);
        }
        let v=self.store.pilot_post_command(r.owner_id,op,json!({"authorization_id":r.authorization_id,"task_id":r.task_id,"receipt_sha256":self.auth.digest(),"account":account,"text_sha256":hash})).await?;
        if v != true {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
#[async_trait]
impl MockDispatchGuard for Guard<'_> {
    async fn check_reserved(&self, h: &str, a: &MockAccount) -> Result<()> {
        self.check("peek", h, a).await
    }
    async fn authorize_exact(&self, h: &str, a: &MockAccount) -> Result<()> {
        self.check("claim", h, a).await
    }
}
fn draft() -> Result<Vec<u8>> {
    let path = Path::new("/etc/supa-agent/x-worker/pilot-draft.json");
    let m = std::fs::symlink_metadata(path).map_err(|_| Error::Denied)?;
    if !m.is_file()
        || m.file_type().is_symlink()
        || m.uid() != 10001
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
        || m.len() > 16384
    {
        return Err(Error::Denied);
    }
    let file = std::fs::File::open(path).map_err(|_| Error::Denied)?;
    let f = file.metadata().map_err(|_| Error::Denied)?;
    if f.ino() != m.ino() || f.dev() != m.dev() {
        return Err(Error::Denied);
    }
    let mut raw = Vec::new();
    file.take(16385)
        .read_to_end(&mut raw)
        .map_err(|_| Error::Denied)?;
    if raw.len() > 16384 {
        return Err(Error::Denied);
    }
    Ok(raw)
}
pub async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["check-disabled"] {
        println!(
            "{}",
            json!({"paused":true,"external_requests":0,"scheduler":false})
        );
        return Ok(());
    }
    if args.len() != 2 || args[0] != "run-once" {
        return Err(Error::Denied);
    }
    let id: Uuid = args[1].parse().map_err(|_| Error::Denied)?;
    let auth = PilotAuthorization::load(id)?;
    let r = auth.receipt();
    let d = validate_draft(&draft()?, &r.text_sha256, &r.account)?;
    let url = std::env::var("DATABASE_URL").map_err(|_| Error::Denied)?;
    let store = Store::connect_first_post_local(&url, false).await?;
    store.verify_first_post_runtime_role().await?;
    store
        .verify_pilot_cluster(&auth.ledger_cluster_id()?)
        .await?;
    let approval = auth.approval(&d.text)?;
    let reserved = store
        .pilot_post_command(r.owner_id, "reserve", approval)
        .await?;
    let body = json!({"authorization_id":id,"task_id":r.task_id,"receipt_sha256":auth.digest()});
    if reserved["fresh"] != true {
        store
            .pilot_post_command(r.owner_id, "recover", body)
            .await?;
        println!(
            "{}",
            json!({"external_requests":0,"duplicate_or_recovery":true,"automatic_retry":false})
        );
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let guard = Guard {
        store: store.clone(),
        auth: &auth,
        stop: stop.clone(),
    };
    // Cancellation after durable intent is uncertain; never dispatch again on restart.
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| Error::Denied)?;
    let result = tokio::select! {biased;
     _=term.recv()=>{stop.store(true,Ordering::SeqCst);Err(Error::Denied)},
     _=tokio::signal::ctrl_c()=>{stop.store(true,Ordering::SeqCst);Err(Error::Denied)},
     r=auth.dispatch(&d.text,&guard)=>r
    };
    let receipt = result.unwrap_or_else(|_| XReceipt {
        mode: "live".into(),
        state: "uncertain".into(),
        category: "interrupted_or_unconfirmed_no_retry".into(),
        post_id: None,
        http_status: None,
        rate_limit_reset: None,
    });
    let success = receipt.state == "posted";
    store.pilot_post_command(r.owner_id,"settle",json!({"authorization_id":id,"task_id":r.task_id,"receipt_sha256":auth.digest(),"receipt":receipt})).await?;
    println!(
        "{}",
        json!({"state":receipt.state,"post_id":receipt.post_id,"automatic_retry":false,"scheduler":false})
    );
    if !success {
        return Err(Error::Denied);
    }
    Ok(())
}
