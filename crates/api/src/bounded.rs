//! Creator-owned finite deterministic scope. Does not open the legacy agent writer routes.
use super::*;
use supa_persistence::{BoundedCommand, BoundedCreate};
impl AppState {
    pub fn with_bounded_autonomy(mut self) -> Result<Self> {
        if !self.synthetic_factory_enabled
            || !self.supervised_tasks_enabled
            || self.live_work_enabled
            || self.controlled.is_some()
            || self.wallets.is_some()
            || self.store.operational_context().is_some()
        {
            return Err(Error::Denied);
        }
        self.bounded_autonomy_enabled = true;
        Ok(self)
    }
}
pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/factory/agents/{id}/autonomy", get(read).post(write))
}
fn allowed(s: &AppState, p: Principal) -> Result<()> {
    if !s.bounded_autonomy_enabled
        || !s.synthetic_factory_enabled
        || !s.supervised_tasks_enabled
        || p.role != Role::Owner
    {
        return Err(Error::Denied);
    }
    Ok(())
}
async fn read(
    State(s): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> ApiResult {
    allowed(&s, p)?;
    Ok(Json(
        s.store.bounded(p.id, "get", json!({"agent_id":id})).await?,
    ))
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Input {
    Create(BoundedCreate),
    Control(BoundedCommand),
}
async fn write(
    State(s): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<Input>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    allowed(&s, p)?;
    let Json(body) = body.map_err(|_| Error::Invalid)?;
    let (operation, mut b) = match body {
        Input::Create(c) => {
            c.scope.validate()?;
            if c.request_id.is_nil() {
                return Err(Error::Invalid.into());
            }
            ("create".to_owned(), json!(c))
        }
        Input::Control(c) => {
            c.validate()?;
            (c.operation.clone(), json!(c))
        }
    };
    b["agent_id"] = json!(id);
    Ok(Json(s.store.bounded(p.id, &operation, b).await?))
}
