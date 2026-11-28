//! Explicit default-off synthetic task endpoints. Owner identity comes only from auth.
use super::*;
use supa_persistence::{SupervisedCommand, SupervisedCreate};
impl AppState {
    pub fn with_supervised_spending_scope(
        mut self,
        scope: supa_runtime::supervised_reasoning::SupervisedStartupAdmission,
    ) -> Result<Self> {
        if !self.supervised_tasks_enabled
            || !self
                .operators
                .iter()
                .any(|c| c.id == scope.owner() && c.role == Role::Owner)
            || !self
                .credential_deadlines
                .get(&scope.owner())
                .is_some_and(|end| scope.session_covers_scope(*end))
        {
            return Err(Error::Denied);
        }
        self.supervised_spending_scope = Some(Arc::new(scope));
        Ok(self)
    }
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
        .route(
            "/factory/tasks/{id}/spending-consent",
            get(consent_status).post(consent),
        )
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
            std::time::Duration::from_secs(
                if std::env::var("SUPA_SUPERVISED_REAL_MODEL").as_deref() == Ok("true") {
                    30
                } else {
                    4
                },
            ),
            s.runtime.supervised_command(p.id, id, &b),
        )
        .await
        .map_err(|_| Error::Timeout)??,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpendingConsent {
    request_id: Uuid,
    expected_revision: u32,
    input_sha256: String,
    confirmed: bool,
    revoke: bool,
}
async fn consent_status(
    State(s): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> ApiResult {
    allowed(&s, p)?;
    // Default-off deployments without the explicit additive schema remain unaffected.
    if s.supervised_spending_scope.is_none() {
        return Err(Error::Denied.into());
    }
    Ok(Json(
        s.store
            .supervised_spending(p.id, "status", json!({"task_id":id}))
            .await?,
    ))
}
async fn consent(
    State(s): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<SpendingConsent>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    allowed(&s, p)?;
    let Json(b) = body.map_err(|_| Error::Invalid)?;
    let scope = s.supervised_spending_scope.as_ref().ok_or(Error::Denied)?;
    let mut data = json!({"task_id":id,"request_id":b.request_id,"expected_revision":b.expected_revision,"input_sha256":b.input_sha256,"confirmed":b.confirmed});
    let checked = SupervisedCommand {
        request_id: b.request_id,
        operation: "approve".into(),
        expected_revision: b.expected_revision,
        input_sha256: b.input_sha256.clone(),
        confirmed: b.confirmed,
        result_sha256: None,
    }
    .validate();
    if checked.is_err() {
        let _ = s
            .store
            .supervised_spending(p.id, "deny", json!({"task_id":id}))
            .await;
        return Err(Error::Invalid.into());
    }
    if !b.revoke {
        match scope
            .consent_receipt(&s.store, p.id, id, &b.input_sha256)
            .await
        {
            Ok(receipt) => data["receipt"] = receipt,
            Err(_) => {
                s.store
                    .supervised_spending(p.id, "deny", json!({"task_id":id}))
                    .await?;
                return Err(Error::Denied.into());
            }
        }
    }
    Ok(Json(
        s.store
            .supervised_spending(p.id, if b.revoke { "revoke" } else { "issue" }, data)
            .await?,
    ))
}
