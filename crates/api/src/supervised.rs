//! Explicit default-off synthetic task endpoints. Owner identity comes only from auth.
use super::*;
use supa_persistence::{SupervisedCommand, SupervisedCreate};
impl AppState {
    pub fn with_supervised_tasks(mut self) -> Result<Self> {
        if !self.synthetic_factory_enabled
            || self.live_work_enabled
            || self.controlled.is_some()
            || self.wallets.is_some()
            || self.store.operational_context().is_some()
        {
            return Err(Error::Denied);
        }
        self.supervised_tasks_enabled = true;
        Ok(self)
    }
}
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/factory/agents/{id}/tasks", get(list).post(create))
        .route("/factory/tasks/{id}", get(detail))
        .route("/factory/tasks/{id}/command", post(command))
}
fn allowed(s: &AppState, p: Principal) -> Result<()> {
    if !s.supervised_tasks_enabled || !s.synthetic_factory_enabled || p.role != Role::Owner {
        Err(Error::Denied)
    } else {
        Ok(())
    }
}
async fn list(
    State(s): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> ApiResult {
    allowed(&s, p)?;
    Ok(Json(
        s.store
            .supervised(p.id, "list", json!({"agent_id":id}))
            .await?,
    ))
}
async fn detail(
    State(s): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> ApiResult {
    allowed(&s, p)?;
    Ok(Json(
        s.store
            .supervised(p.id, "get", json!({"task_id":id}))
            .await?,
    ))
}
async fn create(
    State(s): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    b: std::result::Result<Json<SupervisedCreate>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    allowed(&s, p)?;
    let Json(b) = b.map_err(|_| Error::Invalid)?;
    Ok(Json(s.store.supervised_create(p.id, id, &b).await?))
}
async fn command(
    State(s): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    b: std::result::Result<Json<SupervisedCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    allowed(&s, p)?;
    let Json(b) = b.map_err(|_| Error::Invalid)?;
    let _permit = if b.operation == "advance" {
        Some(s.gate.try_acquire().map_err(|_| Error::ResourceBudget)?)
    } else {
        None
    };
    Ok(Json(
        tokio::time::timeout(
            std::time::Duration::from_secs(4),
            s.runtime.supervised_command(p.id, id, &b),
        )
        .await
        .map_err(|_| Error::Timeout)??,
    ))
}
