mod bounded;
mod access;
mod avatars;
pub use avatars::image_receipt_valid;
mod autonomy;
mod controlled;
mod economy;
mod ecosystem;
mod factory;
mod live_work;
mod operator_status;
mod supervised;
pub use access::{OperatorCredential, Principal};
use axum::Extension;
use supa_persistence::Role;
mod scheduling;
mod wallet;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    sync::Arc,
};
use supa_domain::*;
use supa_persistence::Store;
use supa_runtime::Runtime;
use tokio::sync::Semaphore;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    synthetic_factory_enabled: bool,
    supervised_tasks_enabled: bool,
    bounded_autonomy_enabled: bool,
    agent_avatars_enabled: bool,
    pub live_work_enabled: bool,
    pub controlled: Option<supa_runtime::controlled::ControlledInference>,
    pub store: Store,
    pub runtime: Runtime,
    token: Arc<String>,
    operators: Arc<Vec<OperatorCredential>>,
    credential_deadlines: Arc<BTreeMap<Uuid, u64>>,
    pub gate: Arc<Semaphore>,
    pub wallets: Option<supa_runtime::wallet::WalletService>,
    /// Identity-only scope for persisted wallet observations; grants no RPC, vault or write access.
    pub wallet_read_operator: Option<Uuid>,
}
impl AppState {
    pub fn new(store: Store, runtime: Runtime, token: String) -> Result<Self> {
        if token.len() < 32
            || token.len() > 256
            || !token.is_ascii()
            || token.bytes().any(|b| !(33..=126).contains(&b))
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            synthetic_factory_enabled: false,
            supervised_tasks_enabled: false,
            bounded_autonomy_enabled: false,
            agent_avatars_enabled: false,
            controlled: None,
            live_work_enabled: false,
            store,
            runtime,
            token: Arc::new(token),
            operators: Arc::new(Vec::new()),
            credential_deadlines: Arc::new(BTreeMap::new()),
            gate: Arc::new(Semaphore::new(1)),
            wallets: None,
            wallet_read_operator: None,
        })
    }
}
pub fn local_address(value: &str) -> Result<SocketAddr> {
    let address: SocketAddr = value.parse().map_err(|_| Error::Invalid)?;
    if !address.ip().is_loopback() {
        return Err(Error::Denied);
    }
    Ok(address)
}
fn token_matches(actual: &str, expected: &str) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    actual
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}
async fn auth(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    // An expired operator token must not fall back to the public service credential.
    let matched = token.and_then(|t| state.operators.iter().find(|c| token_matches(t, &c.token)));
    if let Some(credential) = matched {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs());
        if !access::deadline_valid(state.credential_deadlines.get(&credential.id).copied(), now) {
            return ApiError(Error::Unauthorized).into_response();
        }
    }
    let principal = token
        .and_then(|t| state.operators.iter().find(|c| token_matches(t, &c.token)))
        .map(|c| Principal {
            id: c.id,
            role: c.role,
        });
    if principal.is_none() && !token.is_some_and(|t| token_matches(t, &state.token)) {
        return ApiError(Error::Unauthorized).into_response();
    }
    if let (Some(context), Some(actor)) = (state.store.operational_context(), principal)
        && !(request.method() == axum::http::Method::GET
            && request.uri().path() == "/operator-status")
        && state
            .store
            .require_operation(actor.id, &context.scope, None)
            .await
            .is_err()
    {
        return ApiError(Error::Denied).into_response();
    }
    // Epoch writes require an independently admitted, operation-specific server context.
    if request.method() != axum::http::Method::GET && request.method() != axum::http::Method::HEAD {
        match state.store.application_epoch().await {
            Ok(None) => {}
            Ok(Some(_)) => {
                let Some(actor) = principal else {
                    return ApiError(Error::Denied).into_response();
                };
                let path = request.uri().path();
                let parts: Vec<_> = path.split('/').collect();
                let agent = parts.get(2).and_then(|id| Uuid::parse_str(id).ok());
                let context = state.store.operational_context();
                let required = if path == "/agents" {
                    "agent_creation"
                } else if parts.get(3) == Some(&"cycles") {
                    "offline_execution"
                } else if matches!(
                    parts.get(3),
                    Some(&"schedule" | &"emergency-stop" | &"simulated-funding")
                ) {
                    "offline_control"
                } else if parts.get(3) == Some(&"goals")
                    && context.is_some_and(|c| c.scope == "agent_creation")
                {
                    "agent_creation"
                } else {
                    return ApiError(Error::Denied).into_response();
                };
                if state
                    .store
                    .require_operation(actor.id, required, agent)
                    .await
                    .is_err()
                {
                    return ApiError(Error::Denied).into_response();
                }
            }
            Err(_) => return ApiError(Error::Denied).into_response(),
        }
    }
    let path = request.uri().path();
    let public = request.method() == axum::http::Method::GET
        && (matches!(
            path,
            "/health" | "/directory" | "/autonomy" | "/services" | "/public/agent-profiles"
        ) || path.starts_with("/public/agent-profiles/")
            || path.starts_with("/public/agent-avatars/"));
    if !public {
        let Some(actor) = principal else {
            return ApiError(Error::Denied).into_response();
        };
        let write = request.method() != axum::http::Method::GET;
        let parts: Vec<_> = path.split('/').collect();
        let required = if write
            && (parts.get(3) == Some(&"access")
                || parts.get(3) == Some(&"wallet")
                || matches!(
                    parts.get(3),
                    Some(
                        &"purpose"
                            | &"ecosystem-consent"
                            | &"qualification-review"
                            | &"achievement-review"
                    )
                )) {
            Role::Owner
        } else if write {
            Role::Operator
        } else {
            Role::Viewer
        };
        if !actor.role.permits(required) {
            return ApiError(Error::Denied).into_response();
        }
        if parts.get(1) == Some(&"agents")
            && let Some(id) = parts.get(2)
        {
            let Ok(id) = Uuid::parse_str(id) else {
                return ApiError(Error::Denied).into_response();
            };
            if write && state.synthetic_factory_enabled {
                match state.store.factory_managed(id).await {
                    Ok(false) => {}
                    _ => return ApiError(Error::Denied).into_response(),
                }
            }
            if let Err(error) = state.store.require_access(id, actor.id, required).await {
                return ApiError(error).into_response();
            }
            if write
                && parts.get(3) == Some(&"wallet")
                && state
                    .wallets
                    .as_ref()
                    .is_none_or(|s| s.operator() != actor.id)
            {
                return ApiError(Error::Denied).into_response();
            }
        } else if write && parts.get(1) != Some(&"jobs") && !actor.role.permits(Role::Owner) {
            return ApiError(Error::Denied).into_response();
        }
    }
    if let Some(actor) = principal {
        request.extensions_mut().insert(actor);
    }
    next.run(request).await
}
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(access::routes())
        .merge(factory::routes())
        .merge(supervised::routes())
        .merge(bounded::routes())
        .merge(avatars::routes())
        .route("/operator-status", get(operator_status::status))
        .merge(economy::routes())
        .merge(ecosystem::routes())
        .merge(controlled::routes())
        .merge(live_work::routes())
        .merge(wallet::routes())
        .merge(autonomy::routes())
        .merge(scheduling::routes())
        .route("/health", get(health))
        .route("/directory", get(directory))
        .route("/agents", get(list).post(create))
        .route("/agents/{id}", get(status))
        .route("/agents/{id}/lifecycle", post(lifecycle))
        .route("/agents/{id}/pause", post(pause))
        .route("/agents/{id}/resume", post(resume))
        .route("/agents/{id}/emergency-stop", post(emergency_stop))
        .route("/agents/{id}/inference", get(inference))
        .route("/agents/{id}/goal-evaluations", get(evaluations))
        .route("/agents/{id}/goals/{goal}/priority", post(priority))
        .route("/agents/{id}/goals", get(goals).post(add_goal))
        .route("/agents/{id}/goals/{goal}/complete", post(complete_goal))
        .route("/agents/{id}/memory", get(memory))
        .route("/agents/{id}/activity", get(activity))
        .route("/agents/{id}/financials", get(financials))
        .route("/agents/{id}/simulated-funding", post(fund))
        .route("/agents/{id}/cycles", get(cycles).post(cycle))
        .route("/agents/{id}/actions", get(actions))
        .route("/agents/{id}/drafts", get(drafts))
        .route("/agents/{id}/social", get(social))
        .route("/agents/{id}/transitions", get(transitions))
        .route("/agents/{id}/simulated-ledger", get(ledger))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth))
        .route("/ready", get(ready))
        .with_state(state)
}
pub struct ApiError(Error);
impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0 {
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::Denied | Error::FinancialPolicy => StatusCode::FORBIDDEN,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Invalid | Error::InvalidPayment => StatusCode::BAD_REQUEST,
            Error::Unconfirmed => StatusCode::CONFLICT,
            Error::Busy
            | Error::Inactive
            | Error::InvalidTransition
            | Error::Stale
            | Error::InsufficientBalance
            | Error::LiveBudget
            | Error::ResourceBudget
            | Error::SocialRepetition => StatusCode::CONFLICT,
            Error::Timeout => StatusCode::GATEWAY_TIMEOUT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        (status, Json(json!({"error":self.0.to_string()}))).into_response()
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;
// Separate, unauthenticated readiness endpoint: no agent/configuration data.
async fn ready(State(s): State<AppState>) -> (StatusCode, Json<Value>) {
    if s.store.ping().await.is_ok() {
        (StatusCode::OK, Json(json!({"status":"ready"})))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"unavailable"})),
        )
    }
}
async fn health(State(s): State<AppState>) -> ApiResult {
    s.store.ping().await?;
    Ok(Json(
        json!({"status":"ok","economic_mode":"simulation","publishing_enabled":false,"epoch":s.store.application_epoch().await?,"historical_database":"unrecovered"}),
    ))
}
async fn directory(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(s.store.list().await?.into_iter().map(|a|json!({"id":a.id,"name":a.name,"lifecycle":a.lifecycle,"created_at":a.created_at})).collect::<Vec<_>>())))
}
async fn list(State(s): State<AppState>, Extension(actor): Extension<Principal>) -> ApiResult {
    Ok(Json(json!(s.store.accessible_agents(actor.id).await?)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    name: String,
    #[serde(default)]
    personality: Personality,
    #[serde(default)]
    config: CycleConfig,
    #[serde(default)]
    capabilities: BTreeSet<Capability>,
}
async fn create(
    State(s): State<AppState>,
    Extension(actor): Extension<Principal>,
    body: std::result::Result<Json<Create>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    let Json(b) = body.map_err(|_| Error::Invalid)?;
    let mut a = Agent::new(b.name, b.personality, b.config, b.capabilities)?;
    if let Some(context) = s.store.operational_context() {
        a.id = context.agent_id;
    }
    s.store.create_owned(&a, actor.id).await?;
    Ok(Json(json!(a)))
}
async fn status(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
    Ok(Json(
        json!({"agent":s.store.get(id).await?,"financials":s.store.financials(id).await?}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transition {
    state: Lifecycle,
    reason: String,
}
async fn lifecycle(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<Transition>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    let Json(b) = body.map_err(|_| Error::Invalid)?;
    s.store.transition(id, b.state, &b.reason, true).await?;
    Ok(Json(json!({"agent":s.store.get(id).await?})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    reason: String,
}
macro_rules! lifecycle_control {
    ($name:ident,$next:expr)=>{
        async fn $name(State(s):State<AppState>,Path(id):Path<Uuid>,body:std::result::Result<Json<Control>,axum::extract::rejection::JsonRejection>)->ApiResult {
            let Json(b)=body.map_err(|_|Error::Invalid)?;
            s.store.transition(id,$next,&b.reason,true).await?;
            Ok(Json(json!({"agent":s.store.get(id).await?})))
        }
    };
}
lifecycle_control!(pause, Lifecycle::Paused);
lifecycle_control!(resume, Lifecycle::Active);
async fn emergency_stop(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<Control>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    let Json(b) = body.map_err(|_| Error::Invalid)?;
    s.store.emergency_stop(id, &b.reason).await?;
    Ok(Json(json!({"agent":s.store.get(id).await?})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Priority {
    priority: u8,
}
async fn priority(
    State(s): State<AppState>,
    Path((id, goal)): Path<(Uuid, Uuid)>,
    body: std::result::Result<Json<Priority>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    let Json(b) = body.map_err(|_| Error::Invalid)?;
    s.store.set_goal_priority(id, goal, b.priority).await?;
    Ok(Json(json!({"updated":true})))
}
async fn goals(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
    Ok(Json(json!(s.store.get(id).await?.goals)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoalInput {
    description: String,
}
async fn add_goal(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<GoalInput>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    let Json(b) = body.map_err(|_| Error::Invalid)?;
    Ok(Json(json!(s.store.add_goal(id, b.description).await?)))
}
async fn complete_goal(
    State(s): State<AppState>,
    Path((id, goal)): Path<(Uuid, Uuid)>,
) -> ApiResult {
    s.store.complete_goal(id, goal).await?;
    Ok(Json(json!({"completed":true})))
}
async fn memory(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
    Ok(Json(json!(s.store.get(id).await?.memory)))
}
async fn activity(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
    s.store.get(id).await?;
    Ok(Json(json!(s.store.events(id).await?)))
}
async fn financials(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
    Ok(Json(json!(s.store.financials(id).await?)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Funding {
    kind: LedgerKind,
    amount_microusd: i64,
    idempotency_key: Uuid,
    simulated: bool,
}
async fn fund(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<Funding>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    let Json(b) = body.map_err(|_| Error::Invalid)?;
    if !b.simulated {
        return Err(Error::Invalid.into());
    }
    s.store
        .credit(id, b.kind, b.amount_microusd, b.idempotency_key)
        .await?;
    Ok(Json(json!(s.store.financials(id).await?)))
}
async fn cycle(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
    // HTTP execution is bounded/offline only; legacy unknown approvals must be renewed first.
    if s.store.schedule_status(id).await?.policy.is_none()
        || !s
            .store
            .authorization_history(id)
            .await?
            .iter()
            .any(|a| a["closed_at"].is_null() && !a["approved_by"].is_null())
    {
        return Err(Error::Denied.into());
    }
    let _permit = s.gate.try_acquire().map_err(|_| Error::Busy)?;
    let cycle = s.runtime.cycle(id, false).await?;
    Ok(Json(json!({"cycle_id":cycle})))
}
macro_rules! history_handler {
    ($name:ident,$resource:literal) => {
        async fn $name(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
            Ok(Json(json!(s.store.history(id, $resource).await?)))
        }
    };
}
history_handler!(inference, "inference");
history_handler!(evaluations, "evaluations");
history_handler!(cycles, "cycles");
history_handler!(actions, "actions");
history_handler!(drafts, "drafts");
history_handler!(social, "social");
history_handler!(transitions, "transitions");
history_handler!(ledger, "ledger");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_binds_publicly() {
        assert!(local_address("0.0.0.0:8080").is_err());
        assert!(local_address("[::]:8080").is_err());
        assert!(local_address("127.0.0.1:8080").is_ok());
        assert!(local_address("[::1]:8080").is_ok());
    }
    #[test]
    fn token_comparison() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("abc", "abd"));
        assert!(!token_matches("ab", "abc"));
    }
}
