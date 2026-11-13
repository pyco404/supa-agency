mod bounded;
pub use bounded::{BoundedCommand, BoundedCreate, BoundedScope};
mod access;
mod avatars;
mod factory;
mod supervised;
pub use factory::FactoryRequest;
pub use supervised::{SupervisedCommand, SupervisedCreate, SupervisedInput};
mod controlled;
mod economy;
mod ecosystem_reviews;
mod ecosystem_social;
pub use ecosystem_reviews::EcosystemAchievementRequest;
pub use ecosystem_social::EcosystemConsentRequest;
mod epoch;
mod intelligence;
mod operational;
mod operator_status;
mod work_review;
pub use access::Role;
pub use epoch::ApplicationEpoch;
pub use operational::OperationalContext;
mod health;
mod scheduling;
mod social;
mod wallet;
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use supa_domain::*;
use uuid::Uuid;

type Tx<'a> = Transaction<'a, Postgres>;
/// Quarantined after loss of the original cluster. No automatic recreation or settlement.
/// Removing this gate requires an explicitly reviewed historical reconciliation.
fn writable_inference_budget(id: Option<Uuid>) -> Result<()> {
    if id == Some(uuid::uuid!("ad91e475-f1da-495a-8247-139643d2d8b1")) {
        return Err(Error::LiveBudget);
    }
    Ok(())
}
fn db(_: sqlx::Error) -> Error {
    Error::Database
}
fn decode(v: Value) -> Result<Agent> {
    serde_json::from_value(v).map_err(|_| Error::Database)
}
fn encode(a: &Agent) -> Result<Value> {
    serde_json::to_value(a).map_err(|_| Error::Invalid)
}
#[derive(Clone)]
pub struct Store {
    pool: PgPool,
    operational: Option<std::sync::Arc<OperationalContext>>,
    status_pool: Option<PgPool>,
}
impl Store {
    pub async fn connect(url: &str) -> Result<Self> {
        Ok(Self {
            operational: None,
            status_pool: None,
            pool: PgPoolOptions::new()
                .max_connections(8)
                .acquire_timeout(std::time::Duration::from_secs(5))
                .connect(url)
                .await
                .map_err(db)?,
        })
    }
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("../../migrations")
            .run(&self.pool)
            .await
            .map_err(|_| Error::Database)
    }
    /// Staging processes verify an administratively prepared schema without DDL.
    /// Never repair mismatches or accept a runtime login with migration authority.
    pub async fn verify_runtime_schema(&self) -> Result<()> {
        let privileged: bool = sqlx::query_scalar(
            "SELECT rolsuper OR rolcreatedb OR rolcreaterole OR rolbypassrls
             OR has_schema_privilege(current_user,'public','CREATE')
             OR has_table_privilege(current_user,'public._sqlx_migrations','INSERT,UPDATE,DELETE,TRUNCATE')
             FROM pg_roles WHERE rolname=current_user",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(db)?;
        if privileged {
            return Err(Error::Denied);
        }
        let rows = sqlx::query(
            "SELECT version,checksum,success FROM public._sqlx_migrations ORDER BY version",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db)?;
        let migrations = sqlx::migrate!("../../migrations");
        if rows.len() != migrations.iter().count() {
            return Err(Error::Database);
        }
        for (row, expected) in rows.iter().zip(migrations.iter()) {
            if row.try_get::<i64, _>("version").map_err(db)? != expected.version
                || row.try_get::<Vec<u8>, _>("checksum").map_err(db)? != expected.checksum.as_ref()
                || !row.try_get::<bool, _>("success").map_err(db)?
            {
                return Err(Error::Database);
            }
        }
        Ok(())
    }
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(db)?;
        Ok(())
    }
    pub async fn create(&self, a: &Agent) -> Result<()> {
        self.create_with_owner(a, None).await
    }
    pub async fn create_owned(&self, a: &Agent, owner: Uuid) -> Result<()> {
        if owner.is_nil() {
            return Err(Error::Invalid);
        }
        self.create_with_owner(a, Some(owner)).await
    }
    async fn create_with_owner(&self, a: &Agent, owner: Option<Uuid>) -> Result<()> {
        let mut tx = self.operational_transaction().await?;
        sqlx::query("INSERT INTO agents(id,data) VALUES($1,$2)")
            .bind(a.id)
            .bind(encode(a)?)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("INSERT INTO lifecycle_transitions(agent_id,to_state,reason,actor) VALUES($1,'created','operator_created','operator')").bind(a.id).execute(&mut *tx).await.map_err(db)?;
        if let Some(owner) = owner {
            access::assign(&mut tx, a.id, owner, Role::Owner, owner).await?;
        }
        event(&mut tx, a.id, None, "lifecycle", "created", false).await?;
        tx.commit().await.map_err(db)
    }
    pub async fn get(&self, id: Uuid) -> Result<Agent> {
        let v: Option<Value> = sqlx::query_scalar("SELECT data FROM agents WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db)?;
        decode(v.ok_or(Error::NotFound)?)
    }
    pub async fn list(&self) -> Result<Vec<Agent>> {
        let values: Vec<Value> =
            sqlx::query_scalar("SELECT data FROM agents ORDER BY id LIMIT 100")
                .fetch_all(&self.pool)
                .await
                .map_err(db)?;
        values.into_iter().map(decode).collect()
    }
    pub async fn due_agents(&self) -> Result<Vec<Uuid>> {
        sqlx::query_scalar("SELECT a.id FROM agents a JOIN agent_schedules s ON s.agent_id=a.id WHERE a.data->>'lifecycle'='active' AND a.data->'config'->>'scheduler_enabled'='true' AND s.enabled AND EXISTS(SELECT 1 FROM schedule_authorizations h WHERE h.id=s.authorization_id AND h.approved_by IS NOT NULL) AND EXISTS(SELECT 1 FROM agent_access o WHERE o.agent_id=a.id AND o.role='owner') AND (a.next_cycle_at<=now() OR s.expires_at<=now()) ORDER BY a.next_cycle_at LIMIT 100").fetch_all(&self.pool).await.map_err(db)
    }
    pub async fn transition(
        &self,
        id: Uuid,
        next: Lifecycle,
        reason: &str,
        authorized: bool,
    ) -> Result<()> {
        if !authorized {
            return Err(Error::Unauthorized);
        }
        clean_text(reason, 500)?;
        let mut tx = self.operational_transaction().await?;
        let mut a = locked(&mut tx, id).await?;
        if next == Lifecycle::Active
            && sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM agent_schedules WHERE agent_id=$1)",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?
        {
            return Err(Error::Denied); // Managed reactivation requires revision-checked authorization.
        }
        a.lifecycle.transition(next, authorized)?;
        transition(&mut tx, &mut a, next, reason, "operator").await?;
        if next != Lifecycle::Active {
            sqlx::query("UPDATE cycles SET lease_until=now()-interval '1 second' WHERE agent_id=$1 AND status='running'").bind(id).execute(&mut *tx).await.map_err(db)?;
            recover_agent(&mut tx, id).await?;
        }
        tx.commit().await.map_err(db)
    }
    pub async fn add_goal(&self, id: Uuid, description: String) -> Result<Goal> {
        clean_text(&description, 1000)?;
        let mut tx = self.operational_transaction().await?;
        let mut a = locked(&mut tx, id).await?;
        if a.lifecycle == Lifecycle::Terminated || a.goals.len() >= 100 {
            return Err(Error::Invalid);
        }
        let goal = Goal {
            id: Uuid::new_v4(),
            description,
            completed: false,
            priority: 0,
            suggested_progress_bps: 0,
        };
        a.goals.push(goal.clone());
        save(&mut tx, &a).await?;
        event(&mut tx, id, None, "goal", "added", false).await?;
        tx.commit().await.map_err(db)?;
        Ok(goal)
    }
    pub async fn complete_goal(&self, id: Uuid, goal_id: Uuid) -> Result<()> {
        let mut tx = self.operational_transaction().await?;
        let mut a = locked(&mut tx, id).await?;
        if a.lifecycle == Lifecycle::Terminated {
            return Err(Error::Inactive);
        }
        a.goals
            .iter_mut()
            .find(|g| g.id == goal_id)
            .ok_or(Error::NotFound)?
            .completed = true;
        save(&mut tx, &a).await?;
        event(&mut tx, id, None, "goal", "completed", false).await?;
        tx.commit().await.map_err(db)
    }
    /// Operator-only simulated funding. The model has no path to this operation.
    pub async fn credit(&self, id: Uuid, kind: LedgerKind, amount: i64, key: Uuid) -> Result<()> {
        if !matches!(kind, LedgerKind::Seed | LedgerKind::Revenue) {
            return Err(Error::Denied);
        }
        let mut tx = self.operational_transaction().await?;
        let a = locked(&mut tx, id).await?;
        if a.lifecycle == Lifecycle::Terminated {
            return Err(Error::Inactive);
        }
        ledger(&mut tx, id, None, kind, amount, key).await?;
        tx.commit().await.map_err(db)
    }
    pub async fn begin_cycle(&self, id: Uuid, scheduled: bool) -> Result<Uuid> {
        self.begin_cycle_for(id, scheduled, None).await
    }
    pub async fn begin_cycle_for(
        &self,
        id: Uuid,
        scheduled: bool,
        worker: Option<Uuid>,
    ) -> Result<Uuid> {
        let mut tx = self.operational_transaction().await?;
        let mut a = locked(&mut tx, id).await?;
        if a.lifecycle != Lifecycle::Active {
            return Err(Error::Inactive);
        }
        recover_agent(&mut tx, id).await?;
        a = locked(&mut tx, id).await?;
        if a.lifecycle != Lifecycle::Active {
            tx.commit().await.map_err(db)?;
            return Err(Error::Inactive);
        }
        let busy: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM cycles WHERE agent_id=$1 AND status='running')",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if busy {
            return Err(Error::Busy);
        }
        if let Some(error) = scheduling::claim(&mut tx, &mut a, scheduled).await? {
            tx.commit().await.map_err(db)?;
            return Err(error);
        }
        {
            let due: bool =
                sqlx::query_scalar("SELECT next_cycle_at<=now() FROM agents WHERE id=$1")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db)?;
            if !due {
                return Err(Error::Busy);
            }
        }
        if let Some(worker) = worker {
            health::live_worker(&mut tx, worker).await?;
        }
        let cycle = Uuid::new_v4();
        // Maximum runtime is 180s, shorter than this lease. A stale worker is fenced on writes.
        sqlx::query("INSERT INTO cycles(id,agent_id,status,lease_until,worker_id,authorization_id) VALUES($1,$2,'running',now()+interval '240 seconds',$3,(SELECT authorization_id FROM agent_schedules WHERE agent_id=$2))").bind(cycle).bind(id).bind(worker).execute(&mut *tx).await.map_err(db)?;
        sqlx::query("UPDATE agents SET next_cycle_at=now()+make_interval(secs => $2) WHERE id=$1")
            .bind(id)
            .bind(a.config.interval_seconds as f64)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        event(&mut tx, id, Some(cycle), "cycle", "started", false).await?;
        tx.commit().await.map_err(db)?;
        Ok(cycle)
    }
    pub async fn charge(
        &self,
        id: Uuid,
        cycle: Uuid,
        kind: LedgerKind,
        amount: i64,
        key: Uuid,
    ) -> Result<()> {
        if !matches!(kind, LedgerKind::Inference | LedgerKind::Operating) {
            return Err(Error::Denied);
        }
        let mut tx = self.operational_transaction().await?;
        live(&mut tx, id, cycle).await?;
        if amount > 0 {
            ledger(&mut tx, id, Some(cycle), kind, amount, key).await?;
        }
        tx.commit().await.map_err(db)
    }
    /// All proposals are recorded before executing any. Raw model prose is excluded.
    pub async fn propose(&self, id: Uuid, cycle: Uuid, proposals: &[Value]) -> Result<Vec<Uuid>> {
        let mut tx = self.operational_transaction().await?;
        let a = live(&mut tx, id, cycle).await?;
        if proposals.len() > a.config.max_steps {
            return Err(Error::Invalid);
        }
        let mut ids = vec![];
        for (i, p) in proposals.iter().enumerate() {
            let action = Uuid::new_v4();
            sqlx::query("INSERT INTO actions(id,cycle_id,ordinal,proposal,authorization_outcome,status) VALUES($1,$2,$3,$4,'pending','proposed')").bind(action).bind(cycle).bind(i as i32).bind(p).execute(&mut *tx).await.map_err(db)?;
            ids.push(action);
        }
        event(&mut tx, id, Some(cycle), "plan", "proposed", false).await?;
        tx.commit().await.map_err(db)?;
        Ok(ids)
    }
    pub async fn authorize_action(
        &self,
        id: Uuid,
        cycle: Uuid,
        action: Uuid,
        allowed: bool,
    ) -> Result<()> {
        let mut tx = self.operational_transaction().await?;
        live(&mut tx, id, cycle).await?;
        let n=sqlx::query("UPDATE actions SET authorization_outcome=$3,status=$4,finished_at=CASE WHEN $5 THEN NULL ELSE now() END WHERE id=$1 AND cycle_id=$2 AND status='proposed'").bind(action).bind(cycle).bind(if allowed {"allowed"}else{"denied"}).bind(if allowed {"running"}else{"denied"}).bind(allowed).execute(&mut *tx).await.map_err(db)?.rows_affected();
        if n != 1 {
            return Err(Error::Stale);
        }
        event(
            &mut tx,
            id,
            Some(cycle),
            "authorization_outcome",
            if allowed { "allowed" } else { "denied" },
            false,
        )
        .await?;
        tx.commit().await.map_err(db)
    }
    pub async fn execute_action(
        &self,
        id: Uuid,
        cycle: Uuid,
        action: Uuid,
        lesson: Option<&str>,
        draft: Option<&TweetDraft>,
    ) -> Result<()> {
        let mut tx = self.operational_transaction().await?;
        let mut a = live(&mut tx, id, cycle).await?;
        let running:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM actions WHERE id=$1 AND cycle_id=$2 AND status='running' AND authorization_outcome='allowed')").bind(action).bind(cycle).fetch_one(&mut *tx).await.map_err(db)?;
        if !running {
            return Err(Error::Stale);
        }
        let proposal: Value = sqlx::query_scalar("SELECT proposal FROM actions WHERE id=$1")
            .bind(action)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        let sources: Vec<Uuid> =
            serde_json::from_value(proposal.get("event_ids").cloned().unwrap_or(json!([])))
                .map_err(|_| Error::Invalid)?;
        let verified = events_by_ids_tx(&mut tx, id, &sources).await?;
        if sources.len() > 8
            || sources
                .iter()
                .any(|id| !verified.iter().any(|e| e.id == *id))
        {
            return Err(Error::Invalid);
        }
        if let Some(lesson) = lesson {
            clean_text(lesson, 1000)?;
            if a.memory.len() >= 1000 {
                a.memory.remove(0);
            }
            a.memory.push(Memory {
                id: Uuid::new_v4(),
                lesson: lesson.into(),
                kind: MemoryKind::Hypothesis,
                source_event_ids: sources,
                cycle_id: cycle,
                created_at: Utc::now(),
            });
            save(&mut tx, &a).await?;
        }
        if let Some(d) = draft {
            let events = events_by_ids_tx(&mut tx, id, &d.event_ids).await?;
            d.validate(&events)?;
            sqlx::query("INSERT INTO tweet_drafts(id,agent_id,cycle_id,action_id,data) VALUES($1,$2,$3,$4,$5)").bind(Uuid::new_v4()).bind(id).bind(cycle).bind(action).bind(serde_json::to_value(d).map_err(|_|Error::Invalid)?).execute(&mut *tx).await.map_err(db)?;
        }
        sqlx::query("UPDATE actions SET status='succeeded',finished_at=now() WHERE id=$1")
            .bind(action)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        event(&mut tx, id, Some(cycle), "tool", "succeeded", false).await?;
        tx.commit().await.map_err(db)
    }
    pub async fn finish(&self, id: Uuid, cycle: Uuid, failure: Option<&Error>) -> Result<()> {
        let mut tx = self.operational_transaction().await?;
        let mut a = locked(&mut tx, id).await?;
        if let Some(worker) = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT worker_id FROM cycles WHERE id=$1 AND agent_id=$2",
        )
        .bind(cycle)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .flatten()
        {
            health::live_worker(&mut tx, worker).await?;
        }
        let code = failure.map(ToString::to_string);
        let n=sqlx::query("UPDATE cycles SET status=$3,finished_at=now(),failure_code=$4 WHERE id=$1 AND agent_id=$2 AND status='running' AND lease_until>clock_timestamp()").bind(cycle).bind(id).bind(if failure.is_some(){"failed"}else{"completed"}).bind(&code).execute(&mut *tx).await.map_err(db)?.rows_affected();
        if n != 1 {
            return Err(Error::Stale);
        }
        sqlx::query("UPDATE actions SET status='failed',finished_at=now(),failure_code=$2 WHERE cycle_id=$1 AND status IN ('running','proposed')").bind(cycle).bind(code.as_deref().unwrap_or("cycle_ended")).execute(&mut *tx).await.map_err(db)?;
        if matches!(
            failure,
            Some(Error::InsufficientBalance | Error::LiveBudget | Error::ResourceBudget)
        ) && a.lifecycle == Lifecycle::Active
        {
            transition(
                &mut tx,
                &mut a,
                Lifecycle::Suspended,
                code.as_deref().unwrap_or("resource_budget"),
                "system",
            )
            .await?;
        }
        scheduling::finish(&mut tx, &mut a, failure).await?;
        event(
            &mut tx,
            id,
            Some(cycle),
            "cycle",
            code.as_deref().unwrap_or("completed"),
            false,
        )
        .await?;
        if !matches!(failure, Some(Error::Inactive)) {
            let source:Uuid=sqlx::query_scalar("SELECT id FROM activity WHERE agent_id=$1 AND cycle_id=$2 AND kind='cycle' ORDER BY created_at DESC LIMIT 1").bind(id).bind(cycle).fetch_one(&mut *tx).await.map_err(db)?;
            if a.memory.len() >= 1000 {
                a.memory.remove(0);
            }
            a.memory.push(Memory {
                id: Uuid::new_v4(),
                lesson: format!(
                    "Verified cycle {} outcome: {}. Review this outcome before repeating a task.",
                    cycle,
                    code.as_deref().unwrap_or("completed")
                ),
                kind: MemoryKind::VerifiedObservation,
                source_event_ids: vec![source],
                cycle_id: cycle,
                created_at: Utc::now(),
            });
            save(&mut tx, &a).await?;
        }
        tx.commit().await.map_err(db)
    }
    pub async fn recover(&self) -> Result<()> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT agent_id FROM cycles WHERE status='running' AND lease_until<=now()",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db)?;
        for id in ids {
            let mut tx = self.operational_transaction().await?;
            locked(&mut tx, id).await?;
            recover_agent(&mut tx, id).await?;
            tx.commit().await.map_err(db)?;
        }
        Ok(())
    }
    pub async fn events(&self, id: Uuid) -> Result<Vec<Event>> {
        let mut tx = self.operational_transaction().await?;
        let events = events_tx(&mut tx, id).await?;
        tx.commit().await.map_err(db)?;
        Ok(events)
    }
    pub async fn history(&self, id: Uuid, resource: &str) -> Result<Vec<Value>> {
        // Closed query selection: no interpolated identifiers supplied by HTTP callers.
        let query = match resource {
            "cycles" => {
                "SELECT to_jsonb(t) FROM (SELECT * FROM cycles WHERE agent_id=$1 ORDER BY started_at DESC LIMIT 100) t"
            }
            "actions" => {
                "SELECT to_jsonb(t) FROM (SELECT a.* FROM actions a JOIN cycles c ON a.cycle_id=c.id WHERE c.agent_id=$1 ORDER BY a.created_at DESC,a.ordinal LIMIT 100) t"
            }
            "social" => {
                "SELECT to_jsonb(t) FROM (SELECT * FROM social_decisions WHERE agent_id=$1 ORDER BY created_at DESC LIMIT 100) t"
            }
            "drafts" => {
                "SELECT to_jsonb(t) FROM (SELECT * FROM tweet_drafts WHERE agent_id=$1 ORDER BY created_at DESC LIMIT 100) t"
            }
            "transitions" => {
                "SELECT to_jsonb(t) FROM (SELECT * FROM lifecycle_transitions WHERE agent_id=$1 ORDER BY id DESC LIMIT 100) t"
            }
            "inference" => {
                "SELECT to_jsonb(t) FROM (SELECT * FROM inference_calls WHERE agent_id=$1 ORDER BY created_at DESC LIMIT 100) t"
            }
            "evaluations" => {
                "SELECT to_jsonb(t) FROM (SELECT * FROM goal_evaluations WHERE agent_id=$1 ORDER BY created_at DESC LIMIT 100) t"
            }
            "ledger" => {
                "SELECT to_jsonb(t) FROM (SELECT * FROM simulated_ledger WHERE agent_id=$1 ORDER BY created_at DESC LIMIT 100) t"
            }
            _ => return Err(Error::Invalid),
        };
        self.get(id).await?;
        sqlx::query_scalar(query)
            .bind(id)
            .fetch_all(&self.pool)
            .await
            .map_err(db)
    }
    pub async fn financials(&self, id: Uuid) -> Result<Financials> {
        // One statement gives balance and ledger aggregates the same database snapshot.
        let row=sqlx::query("SELECT a.simulated_balance_microusd AS balance, COALESCE(sum(l.amount_microusd) FILTER(WHERE l.kind='revenue'),0)::bigint AS revenue, COALESCE(-sum(l.amount_microusd) FILTER(WHERE l.kind='inference'),0)::bigint AS inference, COALESCE(-sum(l.amount_microusd) FILTER(WHERE l.kind='operating'),0)::bigint AS operating, EXTRACT(EPOCH FROM (now()-min(l.created_at) FILTER(WHERE l.kind IN ('inference','operating'))))::bigint AS elapsed FROM agents a LEFT JOIN simulated_ledger l ON l.agent_id=a.id WHERE a.id=$1 GROUP BY a.id").bind(id).fetch_optional(&self.pool).await.map_err(db)?.ok_or(Error::NotFound)?;
        let balance: i64 = row.get("balance");
        let revenue: i64 = row.get("revenue");
        let inference: i64 = row.get("inference");
        let operating: i64 = row.get("operating");
        let elapsed: Option<i64> = row.get("elapsed");
        Ok(Financials{simulated:true,unit:"simulated_micro_usd".into(),balance_microusd:balance,revenue_microusd:revenue,inference_microusd:inference,operating_microusd:operating,runway_days:runway(balance,inference+operating,elapsed.unwrap_or(0)),runway_assumption:"Lifetime gross simulated expenses / elapsed time since first expense; minimum 60 seconds; excludes future revenue; no expenses or insufficient history => null. Not a forecast of real funds.".into()})
    }
}
async fn locked(tx: &mut Tx<'_>, id: Uuid) -> Result<Agent> {
    let v: Option<Value> = sqlx::query_scalar("SELECT data FROM agents WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db)?;
    decode(v.ok_or(Error::NotFound)?)
}
async fn save(tx: &mut Tx<'_>, a: &Agent) -> Result<()> {
    sqlx::query("UPDATE agents SET data=$2 WHERE id=$1")
        .bind(a.id)
        .bind(encode(a)?)
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    Ok(())
}
async fn live(tx: &mut Tx<'_>, id: Uuid, cycle: Uuid) -> Result<Agent> {
    let a = locked(tx, id).await?;
    if a.lifecycle != Lifecycle::Active {
        return Err(Error::Inactive);
    }
    let permitted: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM agent_schedules WHERE agent_id=$1 AND (NOT enabled OR expires_at<=clock_timestamp()))")
        .bind(id).fetch_one(&mut **tx).await.map_err(db)?;
    if !permitted {
        return Err(Error::Stale);
    }
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cycles WHERE id=$1 AND agent_id=$2 AND status='running' AND lease_until>clock_timestamp() AND (worker_id IS NULL OR EXISTS(SELECT 1 FROM worker_health w WHERE w.id=worker_id AND w.stopped_at IS NULL AND w.heartbeat_at>clock_timestamp()-interval '15 seconds')))").bind(cycle).bind(id).fetch_one(&mut **tx).await.map_err(db)?;
    if !valid {
        return Err(Error::Stale);
    }
    if let Some(worker) =
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT worker_id FROM cycles WHERE id=$1")
            .bind(cycle)
            .fetch_one(&mut **tx)
            .await
            .map_err(db)?
    {
        health::live_worker(tx, worker).await?;
    }
    Ok(a)
}
async fn event(
    tx: &mut Tx<'_>,
    id: Uuid,
    cycle: Option<Uuid>,
    kind: &str,
    code: &str,
    simulated: bool,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO activity(id,agent_id,cycle_id,kind,code,simulated) VALUES($1,$2,$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(cycle)
    .bind(kind)
    .bind(code)
    .bind(simulated)
    .execute(&mut **tx)
    .await
    .map_err(db)?;
    Ok(())
}
async fn transition(
    tx: &mut Tx<'_>,
    a: &mut Agent,
    next: Lifecycle,
    reason: &str,
    actor: &str,
) -> Result<()> {
    let from = serde_json::to_value(a.lifecycle).map_err(|_| Error::Invalid)?;
    let to = serde_json::to_value(next).map_err(|_| Error::Invalid)?;
    sqlx::query("INSERT INTO lifecycle_transitions(agent_id,from_state,to_state,reason,actor) VALUES($1,$2,$3,$4,$5)").bind(a.id).bind(from.as_str()).bind(to.as_str()).bind(reason).bind(actor).execute(&mut **tx).await.map_err(db)?;
    a.lifecycle = next;
    if next != Lifecycle::Active {
        let changed=sqlx::query("UPDATE agent_schedules SET enabled=false,revision=revision+1,updated_at=now() WHERE agent_id=$1").bind(a.id).execute(&mut **tx).await.map_err(db)?.rows_affected();
        if changed > 0 {
            a.config.scheduler_enabled = false;
        }
    }
    save(tx, a).await?;
    event(
        tx,
        a.id,
        None,
        "lifecycle",
        to.as_str().ok_or(Error::Invalid)?,
        false,
    )
    .await
}
async fn ledger(
    tx: &mut Tx<'_>,
    id: Uuid,
    cycle: Option<Uuid>,
    kind: LedgerKind,
    amount: i64,
    key: Uuid,
) -> Result<()> {
    let amount = kind.signed_amount(amount)?;
    let kind = serde_json::to_value(kind).map_err(|_| Error::Invalid)?;
    let kind = kind.as_str().ok_or(Error::Invalid)?;
    let prior=sqlx::query("SELECT amount_microusd,kind,cycle_id FROM simulated_ledger WHERE agent_id=$1 AND idempotency_key=$2").bind(id).bind(key).fetch_optional(&mut **tx).await.map_err(db)?;
    if let Some(row) = prior {
        return if row.get::<i64, _>(0) == amount
            && row.get::<String, _>(1) == kind
            && row.get::<Option<Uuid>, _>(2) == cycle
        {
            Ok(())
        } else {
            Err(Error::Invalid)
        };
    }
    if amount < 0
        && let Some(cycle) = cycle
    {
        let managed: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_schedules WHERE agent_id=$1)")
                .bind(id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db)?;
        if managed {
            let consumed:i64=sqlx::query_scalar("SELECT COALESCE(-sum(amount_microusd),0)::bigint FROM simulated_ledger WHERE agent_id=$1 AND cycle_id=$2 AND amount_microusd<0").bind(id).bind(cycle).fetch_one(&mut **tx).await.map_err(db)?;
            if consumed
                .checked_sub(amount)
                .is_none_or(|total| total > 1100)
            {
                return Err(Error::ResourceBudget);
            }
        }
    }
    let n=sqlx::query("UPDATE agents SET simulated_balance_microusd=simulated_balance_microusd+$2 WHERE id=$1 AND simulated_balance_microusd::numeric+$2::bigint BETWEEN 0 AND 9000000000000000").bind(id).bind(amount).execute(&mut **tx).await.map_err(db)?.rows_affected();
    if n != 1 {
        return Err(if amount < 0 {
            Error::InsufficientBalance
        } else {
            Error::Invalid
        });
    }
    sqlx::query("INSERT INTO simulated_ledger(id,agent_id,cycle_id,kind,amount_microusd,idempotency_key) VALUES($1,$2,$3,$4,$5,$6)").bind(Uuid::new_v4()).bind(id).bind(cycle).bind(kind).bind(amount).bind(key).execute(&mut **tx).await.map_err(db)?;
    event(tx, id, cycle, "simulated_ledger", kind, true).await
}
async fn events_tx(tx: &mut Tx<'_>, id: Uuid) -> Result<Vec<Event>> {
    let rows=sqlx::query("SELECT id,cycle_id,kind,code,simulated,created_at FROM activity WHERE agent_id=$1 ORDER BY created_at DESC,id LIMIT 100").bind(id).fetch_all(&mut **tx).await.map_err(db)?;
    Ok(rows
        .into_iter()
        .map(|r| Event {
            id: r.get(0),
            cycle_id: r.get(1),
            kind: r.get(2),
            code: r.get(3),
            simulated: r.get(4),
            created_at: r.get(5),
        })
        .collect())
}
async fn recover_agent(tx: &mut Tx<'_>, id: Uuid) -> Result<()> {
    let cycles:Vec<Uuid>=sqlx::query_scalar("UPDATE cycles SET status='interrupted',failure_code='lease_expired',finished_at=now() WHERE agent_id=$1 AND status='running' AND lease_until<=now() RETURNING id").bind(id).fetch_all(&mut **tx).await.map_err(db)?;
    if !cycles.is_empty() {
        let mut agent = locked(tx, id).await?;
        let managed: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_schedules WHERE agent_id=$1)")
                .bind(id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db)?;
        if managed {
            scheduling::disable(
                tx,
                &mut agent,
                Lifecycle::Suspended,
                "lease_expired_review_required",
            )
            .await?;
        }
    }
    for cycle in cycles {
        sqlx::query("UPDATE inference_calls SET status='unknown',failure_code='lease_expired',finished_at=now() WHERE cycle_id=$1 AND status='reserved'").bind(cycle).execute(&mut **tx).await.map_err(db)?;
        sqlx::query("UPDATE actions SET status='failed',failure_code='lease_expired',finished_at=now() WHERE cycle_id=$1 AND status IN ('running','proposed')").bind(cycle).execute(&mut **tx).await.map_err(db)?;
        event(tx, id, Some(cycle), "cycle", "interrupted", false).await?;
    }
    Ok(())
}
/// Explicit safe representation of a proposal: excludes private lesson content.
pub fn audit_proposal(tool: &str, action: &ProposedAction) -> Value {
    json!({"tool":tool,"event_ids":action.event_ids,"has_lesson":action.lesson.is_some(),"goal_id":action.evaluation.as_ref().map(|e|e.goal_id)})
}

async fn events_by_ids_tx(tx: &mut Tx<'_>, id: Uuid, ids: &[Uuid]) -> Result<Vec<Event>> {
    if ids.len() > 8 {
        return Err(Error::Invalid);
    }
    let rows=sqlx::query("SELECT id,cycle_id,kind,code,simulated,created_at FROM activity WHERE agent_id=$1 AND id=ANY($2) ORDER BY created_at,id").bind(id).bind(ids).fetch_all(&mut **tx).await.map_err(db)?;
    if rows.len() != ids.len() {
        return Err(Error::Invalid);
    }
    Ok(rows
        .into_iter()
        .map(|r| Event {
            id: r.get(0),
            cycle_id: r.get(1),
            kind: r.get(2),
            code: r.get(3),
            simulated: r.get(4),
            created_at: r.get(5),
        })
        .collect())
}
