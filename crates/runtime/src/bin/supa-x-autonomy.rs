//! Explicit hosted X runner. No startup enrollment, credential export or auto-resume.
use supa_domain::{Error, Result};
use supa_persistence::Store;
use supa_providers::x_review::live::{LiveAuthorization, verify_connection};
use supa_runtime::social_autonomy::{authorize_live, live_control, run_live_scheduler};
use uuid::Uuid;

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2
        || !matches!(
            args[0].as_str(),
            "check" | "authorize" | "resume" | "pause" | "stop" | "revoke" | "read" | "run"
        )
    {
        return Err(Error::Denied);
    }
    let receipt: Uuid = args[1].parse().map_err(|_| Error::Denied)?;
    let auth = LiveAuthorization::load(receipt)?;
    if !auth.is_hosted_oauth1()
        || auth.grant()["max_posts"].as_u64().is_none_or(|n| n > 6)
        || auth.grant()["budget_microusd"]
            .as_u64()
            .is_none_or(|n| n > 150000)
        || auth.expires_unix() > chrono::Utc::now().timestamp().max(0) as u64 + 86400
    {
        return Err(Error::Denied);
    }
    let grant: Uuid = auth.grant()["grant_id"]
        .as_str()
        .ok_or(Error::Denied)?
        .parse()
        .map_err(|_| Error::Denied)?;
    let url = database_url()?;
    let store = Store::connect_first_post_local(&url, false).await?;
    store.verify_first_post_runtime_role().await?;
    store.verify_autonomous_runtime_role().await?;
    match args[0].as_str() {
        "check" => {
            verify_connection(&auth)?;
            println!("X worker preflight passed; no network requests or activation");
        }
        "authorize" => {
            authorize_live(&store, &auth).await?;
            println!("Bounded grant installed paused; no posts");
        }
        "run" => {
            let (tx, rx) = tokio::sync::watch::channel(false);
            let shutdown = tokio::spawn(async move {
                #[cfg(unix)]
                {
                    if let Ok(mut term) =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        tokio::select! {_ = tokio::signal::ctrl_c()=>{},_ = term.recv()=>{}}
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = tokio::signal::ctrl_c().await;
                }
                let _ = tx.send(true);
            });
            let result = run_live_scheduler(&store, &auth, rx, 1440).await;
            shutdown.abort();
            result?;
            println!("Bounded X observation finished; inspect durable audit state");
        }
        operation => {
            let state = live_control(
                &store,
                auth.owner(),
                grant,
                operation,
                if operation == "resume" {
                    Some(&auth)
                } else {
                    None
                },
            )
            .await?;
            if operation == "read" {
                let attempts:Vec<_>=state["attempts"].as_array().into_iter().flatten().map(|a|serde_json::json!({"id":a["id"],"state":a["state"],"post_id":a["receipt"]["post_id"],"category":a["receipt"]["category"]})).collect();
                println!(
                    "{}",
                    serde_json::json!({"state":state["grant"]["state"],"due":state["due"],"attempts":attempts})
                );
            } else {
                println!("X grant control recorded");
            }
        }
    }
    Ok(())
}
// Existing local-only ledger target checks reject Railway/protected databases.
fn database_url() -> Result<String> {
    std::env::var("DATABASE_URL").map_err(|_| Error::Denied)
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "STOPPED: X worker unconfirmed; no automatic dispatch retry. Inspect durable state privately."
        );
        std::process::exit(1);
    }
}
