use std::{sync::Arc, time::Duration};
use supa_api::{AppState, local_address, router};
use supa_domain::{Error, Result};
use supa_persistence::Store;
use supa_providers::{Offline, Reasoner, RoutedOpenAi};
use supa_runtime::Runtime;

#[tokio::main]
async fn main() {
    // Restrict targets: HTTP clients and SQL libraries cannot emit request/connection details.
    tracing_subscriber::fmt()
        .json()
        .with_env_filter("supa_api=info,supa_runtime=info,supa_persistence=info")
        .init();
    if let Err(error) = run().await {
        tracing::error!(error_code=%error,"startup_or_server_failed");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let private_staging = flag("SUPA_PRIVATE_STAGING", false)?;
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let bind = server_address(
        &std::env::var("SUPA_BIND").unwrap_or_else(|_| format!("127.0.0.1:{port}")),
        private_staging,
    )?;
    let scheduler_enabled = flag("SUPA_API_SCHEDULER_ENABLED", false)?;
    let synthetic_factory = flag("SUPA_SYNTHETIC_FACTORY_ENABLED", false)?;
    let supervised_tasks = flag("SUPA_SUPERVISED_TASKS_ENABLED", false)?;
    if supervised_tasks && !synthetic_factory {
        return Err(Error::Denied);
    }
    let bounded_autonomy = flag("SUPA_BOUNDED_AUTONOMY_ENABLED", false)?;
    if bounded_autonomy
        && (!private_staging
            || !synthetic_factory
            || !supervised_tasks
            || flag("SUPA_ALLOW_OPENAI", false)?
            || flag("SUPA_ALLOW_ANTHROPIC", false)?
            || flag("SUPA_SUPERVISED_REAL_MODEL", false)?
            || [
                "SUPA_SUPERVISED_SPENDING_APPROVAL",
                "SUPA_SUPERVISED_REAL_TASK_ID",
                "SUPA_SUPERVISED_REAL_OWNER_ID",
                "SUPA_SUPERVISED_REAL_BUDGET_ID",
            ]
            .iter()
            .any(|k| std::env::var(k).is_ok_and(|v| !v.is_empty())))
    {
        return Err(Error::Denied);
    }
    let agent_avatars = flag("SUPA_AGENT_AVATARS_ENABLED", false)?;
    if agent_avatars && !synthetic_factory {
        return Err(Error::Denied);
    }
    if synthetic_factory
        && (!private_staging || scheduler_enabled || flag("SUPA_WORKER_ENABLED", false)?)
    {
        return Err(Error::Denied);
    }
    let token = std::env::var("SUPA_API_TOKEN").map_err(|_| Error::Invalid)?;
    let provider = std::env::var("SUPA_PROVIDER").unwrap_or_else(|_| "offline".into());
    let credentials: Vec<supa_api::OperatorCredential> = std::env::var("SUPA_OPERATOR_CREDENTIALS")
        .ok()
        .map(|value| serde_json::from_str(&value).map_err(|_| Error::Invalid))
        .transpose()?
        .unwrap_or_default();
    let deadlines: std::collections::BTreeMap<uuid::Uuid, u64> =
        std::env::var("SUPA_OPERATOR_SESSION_DEADLINES")
            .ok()
            .map(|value| serde_json::from_str(&value).map_err(|_| Error::Invalid))
            .transpose()?
            .unwrap_or_default();
    // Only the synthetic task adapter gets this exception. The generic reasoner
    // stays Offline; every other staging restriction still runs below.
    let supervised_admission = if private_staging {
        supa_runtime::supervised_reasoning::SupervisedStartupAdmission::from_settings(|key| {
            std::env::var(key).ok()
        })?
    } else {
        None
    };
    if let Some(scope) = &supervised_admission {
        if !synthetic_factory
            || !supervised_tasks
            || provider != "offline"
            || flag("SUPA_CONTROLLED_WORK", false)?
            || flag("SUPA_WORKER_ENABLED", false)?
            || flag("SUPA_SOCIAL_REVIEW_ENABLED", false)?
            || std::env::var_os("SUPA_WALLET_POLL_SECONDS").is_some()
            || !credentials
                .iter()
                .any(|c| c.id == scope.owner() && c.role == supa_persistence::Role::Owner)
            || !deadlines
                .get(&scope.owner())
                .is_some_and(|deadline| scope.session_covers_scope(*deadline))
        {
            return Err(Error::Denied);
        }
    }
    validate_staging(
        private_staging,
        &provider,
        &[
            scheduler_enabled,
            (flag("SUPA_ALLOW_OPENAI", false)? && supervised_admission.is_none())
                || flag("SUPA_ALLOW_ANTHROPIC", false)?,
            flag("SUPA_CONTROLLED_INFERENCE", false)?,
            flag("SUPA_WALLET_ENABLED", false)?,
            std::env::var("SUPA_OPERATOR_ID").is_ok(),
        ],
        &credentials,
        &token,
    )?;
    let reasoner = configured_provider(&provider, private_staging)?;
    let url = std::env::var("DATABASE_URL").map_err(|_| Error::Invalid)?;
    let store = Store::connect(&url).await?;
    // Identity is checked BEFORE migrations, recovery, heartbeats, or provider calls.
    let epoch = store.application_epoch().await?;
    let expected = std::env::var("SUPA_EXPECTED_EPOCH_ID").ok();
    let preview = flag("SUPA_EPOCH_READONLY_PREVIEW", false)?;
    let epoch_mode = std::env::var("SUPA_EPOCH_MODE").ok();
    if epoch_mode.as_deref() == Some("controlled_offline") {
        if flag("SUPA_CONTROLLED_WORK", false)? {
            return Err(Error::Denied);
        }
        if preview
            || private_staging
            || scheduler_enabled
            || provider != "offline"
            || flag("SUPA_ALLOW_ANTHROPIC", false)?
            || flag("SUPA_ALLOW_OPENAI", false)?
            || flag("SUPA_CONTROLLED_INFERENCE", false)?
            || flag("SUPA_WALLET_ENABLED", false)?
            || std::env::var_os("SUPA_OPERATOR_ID").is_some()
            || std::env::var_os("SUPA_WALLET_POLL_SECONDS").is_some()
        {
            return Err(Error::Denied);
        }
        let id = expected
            .ok_or(Error::Denied)?
            .parse()
            .map_err(|_| Error::Invalid)?;
        let namespace = std::env::var("SUPA_EXPECTED_ACCOUNTING_NAMESPACE")
            .map_err(|_| Error::Denied)?
            .parse()
            .map_err(|_| Error::Invalid)?;
        let approval = std::env::var("SUPA_OPERATION_APPROVAL_ID")
            .map_err(|_| Error::Denied)?
            .parse()
            .map_err(|_| Error::Invalid)?;
        let admitted = store.admit_operation(approval, id, namespace).await?;
        let context = admitted.operational_context().ok_or(Error::Denied)?;
        // One verified principal per admission; frontend identity fields never choose it.
        if credentials.len() != 1
            || credentials[0].id != context.principal_id
            || credentials[0].role != supa_persistence::Role::Owner
            || credentials[0].token == token
            || !deadlines.contains_key(&context.principal_id)
        {
            return Err(Error::Denied);
        }
        let runtime = Runtime::new(admitted.clone(), Arc::new(Offline));
        let state = AppState::new(admitted, runtime, token)?
            .with_credentials(credentials)?
            .with_credential_deadlines(deadlines)?;
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|_| Error::Invalid)?;
        return axum::serve(listener, router(state))
            .with_graceful_shutdown(shutdown())
            .await
            .map_err(|_| Error::Invalid);
    }
    if epoch.is_some() || expected.is_some() || preview || epoch_mode.is_some() {
        let mode = epoch_mode
            .as_deref()
            .unwrap_or(if preview { "preview" } else { "disabled" });
        if !matches!(mode, "preview" | "active_read_only")
            || (mode == "active_read_only" && preview)
            || provider != "offline"
            || scheduler_enabled
            || flag("SUPA_ALLOW_ANTHROPIC", false)?
            || flag("SUPA_ALLOW_OPENAI", false)?
            || flag("SUPA_CONTROLLED_INFERENCE", false)?
            || flag("SUPA_WALLET_ENABLED", false)?
            || private_staging
        {
            return Err(Error::Denied);
        }
        let id = expected
            .ok_or(Error::Denied)?
            .parse()
            .map_err(|_| Error::Invalid)?;
        let approval = std::env::var("SUPA_EPOCH_APPROVAL_ID")
            .ok()
            .map(|value| value.parse().map_err(|_| Error::Invalid))
            .transpose()?;
        store.require_read_only_epoch(id, mode, approval).await?;
        let runtime = Runtime::new(store.clone(), Arc::new(Offline));
        let mut state = AppState::new(store, runtime, token)?;
        if let Ok(value) = std::env::var("SUPA_OPERATOR_CREDENTIALS") {
            state = state
                .with_credentials(serde_json::from_str(&value).map_err(|_| Error::Invalid)?)?;
        }
        state = state.with_credential_deadlines(deadlines)?;
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|_| Error::Invalid)?;
        return axum::serve(listener, router(state))
            .with_graceful_shutdown(shutdown())
            .await
            .map_err(|_| Error::Invalid);
    }
    if private_staging {
        store.verify_runtime_schema().await?;
    } else {
        store.migrate().await?;
    }
    if synthetic_factory {
        // This preview cannot start recovery writers, monitors, workers or schedulers.
        let runtime = Runtime::new(store.clone(), Arc::new(Offline));
        let mut state = AppState::new(store, runtime, token)?
            .with_credentials(credentials)?
            .with_credential_deadlines(deadlines)?
            .with_synthetic_factory()?;
        if agent_avatars {
            state = state.with_agent_avatars()?;
        }
        if supervised_tasks {
            state.store.verify_supervised_schema().await?;
            state = state.with_supervised_tasks()?;
            if let Some(scope) = supervised_admission {
                // Existing ownership and consent checks are read through the
                // restricted command function. No raw grants or recovery writes.
                scope.verify_task(&state.store).await?;
            }
        }
        if bounded_autonomy {
            state.store.verify_bounded_schema(false).await?;
            state = state.with_bounded_autonomy()?;
        }
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|_| Error::Invalid)?;
        return axum::serve(listener, router(state))
            .with_graceful_shutdown(shutdown())
            .await
            .map_err(|_| Error::Invalid);
    }
    store.recover().await?;
    let runtime = Runtime::new(store.clone(), reasoner);
    let mut state = AppState::new(store.clone(), runtime, token)?;
    if let Ok(id) = std::env::var("SUPA_OPERATOR_ID") {
        state = state.with_operator(id.parse().map_err(|_| Error::Invalid)?)?;
    }
    if let Ok(value) = std::env::var("SUPA_OPERATOR_CREDENTIALS") {
        state =
            state.with_credentials(serde_json::from_str(&value).map_err(|_| Error::Invalid)?)?;
    }
    state = state.with_credential_deadlines(deadlines)?;
    if flag("SUPA_CONTROLLED_INFERENCE", false)? {
        if scheduler_enabled || provider != "offline" {
            return Err(Error::Denied);
        }
        state = state.with_controlled_inference(supa_runtime::controlled::ControlledInference {
            store: store.clone(),
            provider: supa_providers::controlled::configured_provider()?,
        });
    }
    store.recover_live_sessions().await?;
    state.wallet_read_operator = std::env::var("SUPA_WALLET_READ_OPERATOR_ID")
        .ok()
        .map(|value| value.parse().map_err(|_| Error::Invalid))
        .transpose()?;
    if std::env::var("SUPA_WALLET_ENABLED").as_deref() == Ok("true") {
        let operator = std::env::var("SUPA_WALLET_OPERATOR_ID")
            .map_err(|_| Error::Invalid)?
            .parse()
            .map_err(|_| Error::Invalid)?;
        let root = std::env::var("SUPA_WALLET_VAULT_DIR").map_err(|_| Error::Invalid)?;
        let master = std::env::var("SUPA_WALLET_MASTER_FILE").map_err(|_| Error::Invalid)?;
        let endpoint = std::env::var("SUPA_SOLANA_RPC_URL").map_err(|_| Error::Invalid)?;
        let vault = supa_providers::vault::KeyVault::from_files(
            std::path::Path::new(&root),
            std::path::Path::new(&master),
        )?;
        let rpc = supa_providers::solana::HttpSolanaRpc::new(
            &endpoint,
            supa_domain::SolanaNetwork::Devnet,
        )?;
        state.wallets = Some(supa_runtime::wallet::WalletService::new(
            store,
            vault,
            Arc::new(rpc),
            operator,
        ));
    }
    let wallet_poll = std::env::var("SUPA_WALLET_POLL_SECONDS")
        .ok()
        .map(|v| v.parse::<u64>().map_err(|_| Error::Invalid))
        .transpose()?;
    if wallet_poll.is_some_and(|v| v < 30) || wallet_poll.is_some() && state.wallets.is_none() {
        return Err(Error::Invalid);
    }
    if flag("SUPA_CONTROLLED_WORK", false)? {
        if state.controlled.is_none() || scheduler_enabled || provider != "offline" {
            return Err(Error::Denied);
        }
        state = state.with_live_work_evaluation();
    }
    let monitor = supa_runtime::health::WorkerMonitor::start(
        state.store.clone(),
        "api",
        scheduler_enabled,
        1,
    )
    .await?;
    let mut scheduler_state = state.clone();
    scheduler_state.runtime = scheduler_state.runtime.with_worker(monitor.id);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let mut wallet_shutdown = shutdown_rx.clone();
    let wallet_state = state.clone();
    let wallet_scheduler=wallet_poll.map(|seconds|tokio::spawn(async move {
        let mut interval=tokio::time::interval(Duration::from_secs(seconds));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {tokio::select! {
            _=wallet_shutdown.changed()=>break,
            _=interval.tick()=>{
                if let Some(service)=&wallet_state.wallets {
                    match wallet_state.store.list().await {
                        Ok(agents)=>for agent in agents {
                            if let Ok(wallet)=wallet_state.store.wallet(agent.id).await
                                && wallet.owner_id==service.operator()
                                && let Err(error)=service.sync(agent.id,service.operator()).await {
                                tracing::warn!(agent_id=%agent.id,error_code=%error,"wallet_monitor_failed");
                            }
                        },
                        Err(error)=>tracing::warn!(error_code=%error,"wallet_monitor_failed"),
                    }
                }
            }
        }}
    }));
    let scheduler = scheduler_enabled.then(|| {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _=shutdown_rx.changed()=>break,
                    _=interval.tick()=>{
                        if let Ok(_permit)=scheduler_state.gate.try_acquire()
                            && let Err(e)=scheduler_state.runtime.tick().await {
                            tracing::warn!(error_code=%e,"scheduler_failed");
                        }
                    }
                }
            }
        })
    });
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| Error::Invalid)?;
    tracing::info!(address=%bind,economic_mode="simulation",provider=provider.as_str(),"local_api_ready");
    let result = axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            shutdown().await;
            let _ = shutdown_tx.send(true);
        })
        .await
        .map_err(|_| Error::Invalid);
    if let Some(task) = wallet_scheduler {
        task.abort();
        let _ = task.await;
    }
    if let Some(task) = scheduler {
        task.abort();
        let _ = task.await;
    }
    monitor.shutdown().await?;
    result
}

fn configured_provider(name: &str, private_staging: bool) -> Result<Arc<dyn Reasoner>> {
    if private_staging && name != "offline" {
        return Err(Error::Denied);
    }
    match name {
        "offline" => Ok(Arc::new(Offline)),
        "openai" => Ok(Arc::new(RoutedOpenAi::from_env()?)),
        _ => Err(Error::Invalid),
    }
}
fn validate_staging(
    private: bool,
    provider: &str,
    unsafe_flags: &[bool],
    credentials: &[supa_api::OperatorCredential],
    service_token: &str,
) -> Result<()> {
    if private
        && (provider != "offline"
            || unsafe_flags.iter().any(|enabled| *enabled)
            || !(32..=256).contains(&service_token.len())
            || service_token.bytes().any(|b| !(33..=126).contains(&b))
            || credentials.is_empty()
            || credentials.len() > 100
            || credentials.iter().any(|c| {
                c.id.is_nil()
                    || c.token == service_token
                    || !(32..=256).contains(&c.token.len())
                    || c.token.bytes().any(|b| !(33..=126).contains(&b))
            })
            || credentials
                .iter()
                .map(|c| c.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != credentials.len()
            || credentials
                .iter()
                .map(|c| &c.token)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != credentials.len())
    {
        return Err(Error::Denied);
    }
    Ok(())
}
fn flag(name: &str, fallback: bool) -> Result<bool> {
    parse_flag(std::env::var(name).ok().as_deref(), fallback)
}
fn parse_flag(value: Option<&str>, fallback: bool) -> Result<bool> {
    match value {
        None => Ok(fallback),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Err(Error::Invalid),
    }
}
fn server_address(value: &str, private_staging: bool) -> Result<std::net::SocketAddr> {
    if private_staging {
        value.parse().map_err(|_| Error::Invalid)
    } else {
        local_address(value)
    }
}
async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = signal.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn staging_fails_closed_before_database_access_or_paid_provider_creation() {
        let credentials = vec![supa_api::OperatorCredential {
            id: uuid::Uuid::new_v4(),
            role: supa_persistence::Role::Viewer,
            token: "fixture-scoped-operator-credential-000000".into(),
        }];
        let service = "fixture-public-service-credential-0000000";
        assert!(validate_staging(true, "offline", &[false; 5], &credentials, service).is_ok());
        for index in 0..5 {
            let mut flags = [false; 5];
            flags[index] = true;
            assert!(validate_staging(true, "offline", &flags, &credentials, service).is_err());
        }
        assert!(validate_staging(true, "openai", &[false; 5], &credentials, service).is_err());
        assert!(validate_staging(true, "offline", &[false; 5], &[], service).is_err());
        assert!(
            validate_staging(
                true,
                "offline",
                &[false; 5],
                &credentials,
                &credentials[0].token
            )
            .is_err()
        );
        assert!(validate_staging(false, "offline", &[false; 5], &[], service).is_ok());
        assert!(validate_staging(true, "offline", &[false; 5], &credentials, "short").is_err());
        let mut invalid = credentials.clone();
        invalid[0].id = uuid::Uuid::nil();
        assert!(validate_staging(true, "offline", &[false; 5], &invalid, service).is_err());
        invalid = credentials.clone();
        invalid[0].token = "short".into();
        assert!(validate_staging(true, "offline", &[false; 5], &invalid, service).is_err());
        invalid = vec![credentials[0].clone(), credentials[0].clone()];
        assert!(validate_staging(true, "offline", &[false; 5], &invalid, service).is_err());
    }
    #[test]
    fn container_bind_requires_explicit_private_staging() {
        assert!(server_address("0.0.0.0:8080", false).is_err());
        assert!(server_address("0.0.0.0:8080", true).is_ok());
        assert!(server_address("[::]:8080", true).is_ok());
        assert!(server_address("127.0.0.1:8080", false).is_ok());
    }
    #[test]
    fn staging_api_cannot_select_a_paid_provider() {
        assert!(configured_provider("offline", true).is_ok());
        assert!(matches!(
            configured_provider("openai", true),
            Err(Error::Denied)
        ));
        assert!(configured_provider("unknown", false).is_err());
    }
    #[test]
    fn scheduler_defaults_can_be_disabled_without_ambiguous_flags() {
        assert!(!parse_flag(None, false).unwrap());
        assert!(parse_flag(None, true).unwrap());
        assert!(!parse_flag(Some("false"), true).unwrap());
        assert!(parse_flag(Some("yes"), false).is_err());
    }
}
