//! Dedicated zero-spend deterministic coordinator; never shares the X worker or its credentials.
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use supa_domain::{Error, Result};
use supa_persistence::Store;
use supa_providers::Offline;
use supa_runtime::Runtime;
fn enabled(value: Option<&str>) -> Result<bool> {
    match value {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        _ => Err(Error::Invalid),
    }
}
#[derive(Clone)]
struct Health {
    store: Store,
    failed: Arc<AtomicBool>,
    active: bool,
}
async fn ready(State(h): State<Health>) -> (StatusCode, Json<serde_json::Value>) {
    let ok = !h.failed.load(Ordering::SeqCst) && h.store.ping().await.is_ok();
    (
        if ok {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(
            serde_json::json!({"ready":ok,"paused":!h.active||h.failed.load(Ordering::SeqCst),"mode":"deterministic","paid_requests":0}),
        ),
    )
}
async fn shutdown() {
    #[cfg(unix)]
    if let Ok(mut signal) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! {_ = tokio::signal::ctrl_c()=>{}, _=signal.recv()=>{}};
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter("supa_bounded_worker=info,supa_runtime=info,supa_persistence=info")
        .init();
    if let Err(e) = run().await {
        tracing::error!(error_code=%e,"bounded_worker_stopped_no_retry");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let active = enabled(std::env::var("SUPA_BOUNDED_WORKER_ENABLED").ok().as_deref())?;
    // Even existing paid/X credentials must never be passed to this dedicated process.
    for k in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "X_API_KEY",
        "X_API_SECRET",
        "X_ACCESS_TOKEN",
        "X_ACCESS_SECRET",
        "SUPA_SUPERVISED_SPENDING_APPROVAL",
    ] {
        if std::env::var(k).is_ok_and(|v| !v.is_empty()) {
            return Err(Error::Denied);
        }
    }
    for k in [
        "SUPA_ALLOW_OPENAI",
        "SUPA_ALLOW_ANTHROPIC",
        "SUPA_SUPERVISED_REAL_MODEL",
        "SUPA_WORKER_ENABLED",
        "SUPA_API_SCHEDULER_ENABLED",
        "SUPA_WALLET_ENABLED",
    ] {
        if enabled(std::env::var(k).ok().as_deref())? {
            return Err(Error::Denied);
        }
    }
    if std::env::var("SUPA_PROVIDER").unwrap_or_else(|_| "offline".into()) != "offline" {
        return Err(Error::Denied);
    }
    let store = Store::connect(&std::env::var("DATABASE_URL").map_err(|_| Error::Invalid)?).await?;
    store.verify_bounded_schema(true).await?;
    let runtime = Runtime::new(store.clone(), Arc::new(Offline));
    let once = std::env::args().skip(1).collect::<Vec<_>>();
    if !once.is_empty() {
        if once != ["--once"] || !active {
            return Err(Error::Denied);
        }
        let result = runtime.bounded_tick().await?;
        println!(
            "{}",
            serde_json::json!({"cycle_completed":result.is_some(),"paid_requests":0})
        );
        return Ok(());
    }
    let bind: std::net::SocketAddr = std::env::var("SUPA_BOUNDED_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8082".into())
        .parse()
        .map_err(|_| Error::Invalid)?;
    if !bind.ip().is_loopback() && !enabled(std::env::var("SUPA_PRIVATE_STAGING").ok().as_deref())?
    {
        return Err(Error::Denied);
    }
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| Error::Invalid)?;
    let health = Health {
        store,
        failed: Arc::new(AtomicBool::new(false)),
        active,
    };
    let failed = health.failed.clone();
    let task = active.then(|| {
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_secs(1));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                if let Err(e) = runtime.bounded_tick().await {
                    failed.store(true, Ordering::SeqCst);
                    tracing::error!(error_code=%e,"bounded_scheduler_failed_paused_no_retry");
                    break;
                }
            }
        })
    });
    let result = axum::serve(
        listener,
        Router::new().route("/ready", get(ready)).with_state(health),
    )
    .with_graceful_shutdown(shutdown())
    .await
    .map_err(|_| Error::Invalid);
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_or_disabled_execution_stays_paused() {
        assert!(!enabled(None).unwrap());
        assert!(!enabled(Some("false")).unwrap());
        assert!(enabled(Some("true")).unwrap());
        assert!(enabled(Some("1")).is_err());
    }
}
