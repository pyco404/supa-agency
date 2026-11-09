//! Disposable PG18 only; runtime uses the real scheduler and mock X protocol.
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, postgres::PgConnectOptions};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use supa_domain::{Agent, CycleConfig, Personality, Result};
use supa_persistence::{AutonomousSocialGrant, Store};
use supa_providers::x_review::{MockResponse, MockXTransport};
use supa_runtime::social_autonomy::{draft_released, tick_mock};
use uuid::Uuid;
fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
async fn admin() -> Store {
    let raw = std::env::var("SUPA_AUTONOMY_TEST_URL").expect("explicit_disposable_url_required");
    let o: PgConnectOptions = raw.parse().unwrap();
    assert_eq!(o.get_database(), Some("supa_autonomy_test"));
    assert_eq!(o.get_username(), "supa_autonomy_test");
    assert_eq!(o.get_port(), 55483);
    let socket = o.get_socket().unwrap();
    let parent = socket.parent().unwrap();
    assert_eq!(parent.parent(), Some(std::path::Path::new("/tmp")));
    assert_eq!(socket.file_name().unwrap(), "socket");
    assert!(
        parent
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_prefix("supa-autonomy-test-")
            .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    );
    assert_eq!(socket.canonicalize().unwrap(), *socket);
    let s = Store::connect(o.to_url_lossy().as_str()).await.unwrap();
    let (version,listen,addr):(String,String,Option<String>)=sqlx::query_as("SELECT current_setting('server_version'),current_setting('listen_addresses'),inet_server_addr()::text").fetch_one(s.pool()).await.unwrap();
    assert!(version.starts_with("18."));
    assert!(listen.is_empty());
    assert!(addr.is_none());
    s
}
async fn restricted(s: &Store) -> Store {
    let mut o: PgConnectOptions = std::env::var("SUPA_AUTONOMY_TEST_URL")
        .unwrap()
        .parse()
        .unwrap();
    o = o.username("supa_autonomy_runtime");
    let rt = Store::connect(o.to_url_lossy().as_str()).await.unwrap();
    assert!(
        sqlx::query("SELECT * FROM social_autonomy.grants")
            .fetch_all(rt.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM agent_access")
            .execute(rt.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE social_review.policies SET enabled=false")
            .execute(rt.pool())
            .await
            .is_err()
    );
    let _ = s;
    rt
}
async fn command(s: &Store, op: &str, grant: Uuid) -> Value {
    s.autonomous_social_command_mock(id(1), op, json!({"grant_id":grant}))
        .await
        .unwrap()
}
async fn grant(s: &Store, n: u128, budget: i64) -> Uuid {
    let g = AutonomousSocialGrant {
        grant_id: id(n),
        account: json!({"id":"1","username":"supa_agency","mode":"mock"}),
        confirmed: true,
        scope: "original_text_released_metadata_only".into(),
        expires_at: Utc::now() + Duration::days(1),
        max_posts: 6,
        interval_seconds: 14400,
        budget_microusd: budget,
    };
    let a = s.autonomous_social_authorize_mock(id(1), &g).await.unwrap();
    assert_eq!(a["state"], "paused");
    assert!(s.autonomous_social_authorize_mock(id(2), &g).await.is_err());
    let replay = s.autonomous_social_authorize_mock(id(1), &g).await.unwrap();
    assert_eq!(replay["id"], a["id"]);
    command(s, "resume", g.grant_id).await;
    g.grant_id
}
async fn source_kind(s: &Store, n: u128, kind: &str, code: &str) -> Value {
    sqlx::query("INSERT INTO activity(id,agent_id,kind,code,simulated) VALUES($1,$2,$4,$3,true)")
        .bind(id(n))
        .bind(id(10))
        .bind(code)
        .bind(kind)
        .execute(s.pool())
        .await
        .unwrap();
    let ctx: Value = sqlx::query_scalar("SELECT social_review.context($1)")
        .bind(id(10))
        .fetch_one(s.pool())
        .await
        .unwrap();
    let src = ctx["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["event"]["id"] == json!(id(n)))
        .unwrap();
    sqlx::query("INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released) VALUES($1,$2,$3,$4,true)")
 .bind(id(10)).bind(id(n)).bind(id(1)).bind(src["snapshot_sha256"].as_str().unwrap()).execute(s.pool()).await.unwrap();
    src.clone()
}
async fn source(s: &Store, n: u128, code: &str) -> Value {
    source_kind(s, n, "cycle", code).await
}
#[derive(Clone)]
struct Wire {
    calls: Arc<Mutex<Vec<String>>>,
    mode: &'static str,
    account: &'static str,
    gate: Option<(Store, Uuid, &'static str)>,
}
impl Wire {
    fn new(mode: &'static str) -> Self {
        Self {
            calls: Arc::new(Mutex::new(vec![])),
            mode,
            account: "1",
            gate: None,
        }
    }
}
#[async_trait]
impl MockXTransport for Wire {
    async fn exchange(
        &self,
        path: &str,
        body: Option<Value>,
        credential: &str,
    ) -> Result<MockResponse> {
        assert_eq!(credential, "mock-x-credential-no-real-account");
        self.calls.lock().unwrap().push(path.into());
        if path == "/2/users/me" {
            if let Some((store, grant, op)) = &self.gate {
                command(store, op, *grant).await;
            }
            return Ok(MockResponse{status:if self.mode=="identity_rate"{429}else{200},body:serde_json::to_vec(&json!({"data":{"id":if self.mode=="wrong"{"3"}else{self.account},"username":"supa_agency"}})).unwrap(),rate_limit_reset:None});
        }
        assert_eq!(path, "/2/tweets");
        if self.mode == "lost" {
            return Err(supa_domain::Error::ProviderTransport);
        }
        Ok(MockResponse {
            status: if self.mode == "rate" { 429 } else { 201 },
            body: serde_json::to_vec(&json!({"data":{"id":"123456","text":body.unwrap()["text"]}}))
                .unwrap(),
            rate_limit_reset: None,
        })
    }
}
async fn expire_spacing(admin: &Store, grant: Uuid) {
    sqlx::query("UPDATE social_autonomy.grants SET next_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(grant).execute(admin.pool()).await.unwrap();
    sqlx::query("UPDATE social_autonomy.attempts SET created_at=created_at-interval '5 hours'")
        .execute(admin.pool())
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "explicit new disposable PG18 fixture only"]
async fn journey() {
    let a = admin().await;
    a.migrate().await.unwrap();
    for sql in [
        include_str!("../../../deployment/ecosystem-social.sql"),
        include_str!("../../../deployment/social-review.sql"),
        include_str!("../../../deployment/social-autonomy.sql"),
    ] {
        sqlx::raw_sql(sql).execute(a.pool()).await.unwrap();
    }
    let mut agent = Agent::new(
        "private-agent-name-not-public".into(),
        Personality::default(),
        CycleConfig::default(),
        BTreeSet::new(),
    )
    .unwrap();
    agent.id = id(10);
    a.create_owned(&agent, id(1)).await.unwrap();
    sqlx::query("INSERT INTO social_review.policies(agent_id,owner_id,account,enabled) VALUES($1,$2,'{\"id\":\"1\",\"username\":\"supa_agency\",\"mode\":\"mock\"}',true)").bind(id(10)).bind(id(1)).execute(a.pool()).await.unwrap();
    sqlx::raw_sql("CREATE ROLE supa_autonomy_runtime LOGIN; GRANT USAGE ON SCHEMA social_autonomy TO supa_autonomy_runtime; GRANT EXECUTE ON FUNCTION social_autonomy.command(uuid,text,jsonb), social_autonomy.policy_mode(uuid) TO supa_autonomy_runtime;").execute(a.pool()).await.unwrap();
    let s = restricted(&a).await;
    let g = grant(&s, 100, 250000).await;
    let wire = Wire::new("ok");
    assert_eq!(
        tick_mock(&s, id(1), g, id(1000), wire.clone())
            .await
            .unwrap()["state"],
        "abstained"
    );
    assert!(wire.calls.lock().unwrap().is_empty());
    source(&a, 200, "completed").await;
    expire_spacing(&a, g).await;
    let result = tick_mock(&s, id(1), g, id(1001), wire.clone())
        .await
        .unwrap();
    assert_eq!(result["state"], "posted");
    assert_eq!(wire.calls.lock().unwrap().len(), 2);
    assert_eq!(
        tick_mock(&s, id(1), g, id(1001), wire.clone())
            .await
            .unwrap()["state"],
        "not_due"
    );
    assert!(
        tick_mock(&s, id(2), g, id(2000), Wire::new("ok"))
            .await
            .is_err()
    );
    let state = command(&s, "read", g).await;
    assert_eq!(state["grant"]["reserved"], 25000);
    assert_eq!(state["attempts"][0]["state"], "posted");
    assert!(
        !state["context"]
            .to_string()
            .contains("private-agent-name-not-public")
    );
    // Unreleased input and arbitrary private claims cannot become autonomous text.
    let mut context = state["context"].clone();
    context["sources"] = json!([]);
    assert!(draft_released(&context).is_err());
    // Pause during identity verification fences the subsequent POST.
    source(&a, 201, "interrupted").await;
    expire_spacing(&a, g).await;
    let mut pause = Wire::new("ok");
    pause.gate = Some((s.clone(), g, "pause"));
    assert_eq!(
        tick_mock(&s, id(1), g, id(1002), pause.clone())
            .await
            .unwrap()["state"],
        "failed"
    );
    assert_eq!(pause.calls.lock().unwrap().len(), 1);
    command(&s, "resume", g).await;
    // Ambiguous outcomes are never resubmitted; grant pauses and the budget stays held.
    source_kind(&a, 202, "goal", "evaluated").await;
    expire_spacing(&a, g).await;
    let lost = Wire::new("lost");
    let r = tick_mock(&s, id(1), g, id(1003), lost.clone())
        .await
        .unwrap();
    assert_eq!(r["state"], "uncertain");
    assert_eq!(command(&s, "read", g).await["grant"]["state"], "paused");
    assert_eq!(
        tick_mock(&s, id(1), g, id(1003), lost.clone())
            .await
            .unwrap()["state"],
        "not_due"
    );
    assert_eq!(lost.calls.lock().unwrap().len(), 2);
    assert!(
        s.autonomous_social_command_mock(id(1), "resume", json!({"grant_id":g}))
            .await
            .is_err(),
        "uncertainty cannot be bypassed by resume"
    );
    let replacement = AutonomousSocialGrant {
        grant_id: id(102),
        account: json!({"id":"1","username":"supa_agency","mode":"mock"}),
        confirmed: true,
        scope: "original_text_released_metadata_only".into(),
        expires_at: Utc::now() + Duration::days(1),
        max_posts: 6,
        interval_seconds: 14400,
        budget_microusd: 250000,
    };
    s.autonomous_social_authorize_mock(id(1), &replacement)
        .await
        .unwrap();
    assert!(
        s.autonomous_social_command_mock(id(1), "resume", json!({"grant_id":id(102)}))
            .await
            .is_err()
    );
    // This synthetic provider fixture alone can attest that no remote post exists.
    // There is no production reconcile endpoint or automatic uncertainty clearing.
    sqlx::query("UPDATE social_autonomy.attempts SET state='failed',receipt=$2 WHERE id=$1")
        .bind(id(1003)).bind(json!({"mode":"mock","state":"failed","category":"fixture_independent_no_post_confirmed"})).execute(a.pool()).await.unwrap();
    command(&s, "stop", g).await;
    assert!(
        s.autonomous_social_command_mock(id(1), "resume", json!({"grant_id":g}))
            .await
            .is_err()
    );
    // Persist a reservation without sending it: restart must quarantine it, never dispatch.
    let h = grant(&s, 101, 250000).await;
    source(&a, 204, "completed").await;
    expire_spacing(&a, h).await;
    // Different topic and tail are chosen only if the existing repetition checker permits it.
    // Use a fresh explicitly released goal event as restart evidence.
    sqlx::query(
        "INSERT INTO activity(id,agent_id,kind,code,simulated) VALUES($1,$2,'goal','added',true)",
    )
    .bind(id(205))
    .bind(id(10))
    .execute(a.pool())
    .await
    .unwrap();
    let c: Value = sqlx::query_scalar("SELECT social_review.context($1)")
        .bind(id(10))
        .fetch_one(a.pool())
        .await
        .unwrap();
    let e = c["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["event"]["id"] == json!(id(205)))
        .unwrap();
    sqlx::query("INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released) VALUES($1,$2,$3,$4,true)").bind(id(10)).bind(id(205)).bind(id(1)).bind(e["snapshot_sha256"].as_str().unwrap()).execute(a.pool()).await.unwrap();
    let read = command(&s, "read", h).await;
    let proposal = draft_released(&read["context"]).unwrap();
    let reserved=s.autonomous_social_command_mock(id(1),"reserve",json!({"grant_id":h,"attempt_id":id(1004),"proposal":proposal,"context_sha256":read["context_sha256"]})).await.unwrap();
    assert_eq!(reserved["fresh"], true);
    // Confirm per-account uniqueness even when a new authorization is issued.
    let snapshot = command(&s, "read", h).await;
    assert_eq!(snapshot["attempts"][0]["state"], "reserved");
    for event in snapshot["audit"].as_array().unwrap().windows(2) {
        assert_eq!(event[0]["sha256"], event[1]["previous_sha256"]);
    }
    assert!(
        sqlx::query("DELETE FROM social_autonomy.events")
            .execute(a.pool())
            .await
            .is_err()
    );
}
#[tokio::test]
#[ignore = "same disposable fixture after an actual PG18 restart"]
async fn restart() {
    let a = admin().await;
    let s = restricted(&a).await;
    let state = command(&s, "recover", id(101)).await;
    assert_eq!(state["grant"]["state"], "paused");
    assert_eq!(state["attempts"][0]["state"], "uncertain");
    assert_eq!(state["grant"]["reserved"], 25000);
    let wire = Wire::new("ok");
    assert_eq!(
        tick_mock(&s, id(1), id(101), id(1004), wire.clone())
            .await
            .unwrap()["state"],
        "not_due"
    );
    assert!(wire.calls.lock().unwrap().is_empty());
    let live = s
        .autonomous_social_command_live(id(3), "read", json!({"grant_id":id(30000)}))
        .await
        .unwrap();
    assert_eq!(live["grant"]["state"], "revoked");
    assert_eq!(live["attempts"][0]["state"], "uncertain");
    assert_eq!(live["attempts"][0]["receipt"]["mode"], "live");
    assert_eq!(live["grant"]["reserved"], 25000);
    assert!(
        s.autonomous_social_command_live(id(3), "resume", json!({"grant_id":id(30000)}))
            .await
            .is_err()
    );
}

async fn release_for(a: &Store, agent: Uuid, owner: Uuid, event: Uuid, kind: &str, code: &str) {
    sqlx::query("INSERT INTO activity(id,agent_id,kind,code,simulated) VALUES($1,$2,$3,$4,true)")
        .bind(event)
        .bind(agent)
        .bind(kind)
        .bind(code)
        .execute(a.pool())
        .await
        .unwrap();
    let c: Value = sqlx::query_scalar("SELECT social_review.context($1)")
        .bind(agent)
        .fetch_one(a.pool())
        .await
        .unwrap();
    let src = c["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["event"]["id"] == json!(event))
        .unwrap();
    sqlx::query("INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released) VALUES($1,$2,$3,$4,true)")
 .bind(agent).bind(event).bind(owner).bind(src["snapshot_sha256"].as_str().unwrap()).execute(a.pool()).await.unwrap();
}
#[tokio::test]
#[ignore = "same isolated mock PG18 fixture; no production configuration"]
async fn boundaries() {
    let a = admin().await;
    let s = restricted(&a).await;
    let mut agent = Agent::new(
        "second-private-agent".into(),
        Personality::default(),
        CycleConfig::default(),
        BTreeSet::new(),
    )
    .unwrap();
    agent.id = id(20);
    a.create_owned(&agent, id(2)).await.unwrap();
    sqlx::query("INSERT INTO social_review.policies(agent_id,owner_id,account,enabled) VALUES($1,$2,'{\"id\":\"2\",\"username\":\"supa_agency\",\"mode\":\"mock\"}',true)").bind(id(20)).bind(id(2)).execute(a.pool()).await.unwrap();
    let mut g = AutonomousSocialGrant {
        grant_id: id(500),
        account: json!({"id":"2","username":"supa_agency","mode":"mock"}),
        confirmed: true,
        scope: "original_text_released_metadata_only".into(),
        expires_at: Utc::now() + Duration::days(1),
        max_posts: 6,
        interval_seconds: 14400,
        budget_microusd: 250000,
    };
    for (max, seconds) in [(7, 14400), (0, 14400), (6, 14399)] {
        let mut bad = g.clone();
        bad.max_posts = max;
        bad.interval_seconds = seconds;
        assert!(
            s.autonomous_social_authorize_mock(id(2), &bad)
                .await
                .is_err()
        );
    }
    let mut bad = g.clone();
    bad.account["mode"] = json!("live");
    assert!(
        s.autonomous_social_authorize_mock(id(2), &bad)
            .await
            .is_err()
    );
    bad = g.clone();
    bad.confirmed = false;
    assert!(
        s.autonomous_social_authorize_mock(id(2), &bad)
            .await
            .is_err()
    );
    assert!(s.autonomous_social_authorize_mock(id(1), &g).await.is_err());
    s.autonomous_social_authorize_mock(id(2), &g).await.unwrap();
    let cmd =
        |op: &'static str| s.autonomous_social_command_mock(id(2), op, json!({"grant_id":id(500)}));
    cmd("resume").await.unwrap();
    release_for(&a, id(20), id(2), id(5010), "cycle", "completed").await;
    let mut wrong = Wire::new("wrong");
    wrong.account = "2";
    assert_eq!(
        tick_mock(&s, id(2), id(500), id(5001), wrong.clone())
            .await
            .unwrap()["category"],
        "account_mismatch"
    );
    assert_eq!(wrong.calls.lock().unwrap().len(), 1);
    assert_eq!(cmd("read").await.unwrap()["grant"]["state"], "paused");
    cmd("resume").await.unwrap();
    expire_spacing(&a, id(500)).await;
    release_for(&a, id(20), id(2), id(5011), "cycle", "interrupted").await;
    let mut rate = Wire::new("identity_rate");
    rate.account = "2";
    let limited = tick_mock(&s, id(2), id(500), id(5002), rate.clone())
        .await
        .unwrap();
    assert_eq!(limited["http_status"], 429);
    assert!(limited["rate_limit_reset"].as_i64().unwrap() > Utc::now().timestamp());
    assert_eq!(rate.calls.lock().unwrap().len(), 1);
    expire_spacing(&a, id(500)).await;
    release_for(&a, id(20), id(2), id(5012), "goal", "evaluated").await;
    let mut wire = Wire::new("ok");
    wire.account = "2";
    assert_eq!(
        tick_mock(&s, id(2), id(500), id(5003), wire.clone())
            .await
            .unwrap()["state"],
        "not_due"
    );
    assert!(wire.calls.lock().unwrap().is_empty());
    // Mock passage of the rate-limit window; never change a live clock or real record.
    sqlx::query("UPDATE social_autonomy.attempts SET receipt=jsonb_set(receipt,'{rate_limit_reset}',to_jsonb(extract(epoch from clock_timestamp()-interval '1 hour')::bigint)) WHERE account_id='2' AND receipt->>'http_status'='429'").execute(a.pool()).await.unwrap();
    let (one, two) = tokio::join!(
        tick_mock(&s, id(2), id(500), id(5004), wire.clone()),
        tick_mock(&s, id(2), id(500), id(5005), wire.clone())
    );
    assert_eq!(
        [one.as_ref(), two.as_ref()]
            .iter()
            .filter(|r| r.as_ref().is_ok_and(|v| v["state"] == "posted"))
            .count(),
        1
    );
    assert_eq!(wire.calls.lock().unwrap().len(), 2);
    // Slots derive from current time, not an overdue schedule's previous deadline.
    let read = cmd("read").await.unwrap();
    let due =
        chrono::DateTime::parse_from_rfc3339(read["grant"]["next_at"].as_str().unwrap()).unwrap();
    assert!(due.timestamp() >= Utc::now().timestamp() + 14400);
    let (_sender, receiver) = tokio::sync::watch::channel(true);
    assert_eq!(
        supa_runtime::social_autonomy::run_mock_scheduler(
            &s,
            id(2),
            id(500),
            || Wire::new("ok"),
            receiver,
            1
        )
        .await
        .unwrap(),
        0
    );
    release_for(&a, id(20), id(2), id(5013), "goal", "added").await;
    // Prepare an otherwise valid new proposal and bypass only due time in this synthetic fixture.
    sqlx::query("UPDATE social_autonomy.grants SET next_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(id(500)).execute(a.pool()).await.unwrap();
    let read = cmd("read").await.unwrap();
    let proposal = draft_released(&read["context"]).unwrap();
    let body = json!({"grant_id":id(500),"attempt_id":id(5006),"context_sha256":read["context_sha256"],"proposal":proposal});
    assert!(
        s.autonomous_social_command_mock(id(2), "reserve", body.clone())
            .await
            .is_err(),
        "minimum interval"
    );
    // Fill the rolling window with failed synthetic historical reservations.
    for n in 0..3 {
        let text = format!("Synthetic failed history {n}");
        sqlx::query("INSERT INTO social_autonomy.attempts(id,grant_id,account_id,text,text_sha256,decision,topics,state,created_at) VALUES($1,$2,'2',$3,$4,$5,'[]','failed',clock_timestamp()-make_interval(hours=>$6))")
 .bind(id(5100+n)).bind(id(500)).bind(&text).bind(supa_providers::x_review::text_sha256(&text)).bind(json!({"event_ids":[],"claims":[]})).bind(8+(n as i32)*4).execute(a.pool()).await.unwrap();
    }
    sqlx::query("UPDATE social_autonomy.attempts SET created_at=created_at-interval '5 hours' WHERE account_id='2' AND id IN ($1,$2)").bind(id(5004)).bind(id(5005)).execute(a.pool()).await.unwrap();
    let cap_context = cmd("read").await.unwrap();
    let mut body = body;
    body["context_sha256"] = cap_context["context_sha256"].clone();
    assert!(
        s.autonomous_social_command_mock(id(2), "reserve", body)
            .await
            .is_err(),
        "six slots, including failures"
    );
    // Even a newly authorized grant cannot reset account caps.
    g.grant_id = id(502);
    s.autonomous_social_authorize_mock(id(2), &g).await.unwrap();
    s.autonomous_social_command_mock(id(2), "resume", json!({"grant_id":id(502)}))
        .await
        .unwrap();
    let fresh = s
        .autonomous_social_command_mock(id(2), "read", json!({"grant_id":id(502)}))
        .await
        .unwrap();
    assert!(s.autonomous_social_command_mock(id(2),"reserve",json!({"grant_id":id(502),"attempt_id":id(5200),"proposal":proposal,"context_sha256":fresh["context_sha256"]})).await.is_err());
    // Once the rolling window actually expires in the mock fixture, one slot is available.
    sqlx::query("UPDATE social_autonomy.attempts SET created_at=created_at-interval '25 hours' WHERE account_id='2'").execute(a.pool()).await.unwrap();
    let fresh = s
        .autonomous_social_command_mock(id(2), "read", json!({"grant_id":id(502)}))
        .await
        .unwrap();
    let reserved=s.autonomous_social_command_mock(id(2),"reserve",json!({"grant_id":id(502),"attempt_id":id(5201),"proposal":proposal,"context_sha256":fresh["context_sha256"]})).await.unwrap();
    assert_eq!(reserved["fresh"], true);
    // Release revocation invalidates dispatch, while the slot/budget remains reserved.
    sqlx::query("INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released) SELECT agent_id,event_id,actor,snapshot_sha256,false FROM social_review.releases WHERE event_id=$1 ORDER BY id DESC LIMIT 1").bind(id(5013)).execute(a.pool()).await.unwrap();
    assert!(s.autonomous_social_command_mock(id(2),"guard",json!({"grant_id":id(502),"attempt_id":id(5201),"account":g.account,"text_sha256":proposal["text_sha256"]})).await.is_err());
    s.autonomous_social_command_mock(id(2),"finish",json!({"grant_id":id(502),"attempt_id":id(5201),"receipt":{"mode":"mock","state":"failed","category":"evidence_revoked_before_dispatch"}})).await.unwrap();
    s.autonomous_social_command_mock(id(2), "revoke", json!({"grant_id":id(502)}))
        .await
        .unwrap();
    assert!(
        s.autonomous_social_command_mock(id(2), "resume", json!({"grant_id":id(502)}))
            .await
            .is_err()
    );
    // A reviewed manual post cannot evade the shared account slot fence.
    assert!(sqlx::query("WITH d AS (INSERT INTO social_review.drafts(agent_id,text,text_sha256,decision,topics,account) VALUES($1,'Synthetic manual fixture',encode(sha256(convert_to('Synthetic manual fixture','UTF8')),'hex'),'{}','[]',$2) RETURNING id) INSERT INTO social_review.attempts(agent_id,draft_id,account_id,normalized_text) SELECT $1,id,'2','syntheticmanualfixture' FROM d").bind(id(20)).bind(g.account.clone()).execute(a.pool()).await.is_err());
    // SQL reservations independently enforce the grant budget even if a caller
    // bypasses the Rust draft planner. Nothing is sent for these DB-only probes.
    g.grant_id = id(503);
    g.budget_microusd = 25000;
    s.autonomous_social_authorize_mock(id(2), &g).await.unwrap();
    s.autonomous_social_command_mock(id(2), "resume", json!({"grant_id":id(503)}))
        .await
        .unwrap();
    sqlx::query("UPDATE social_autonomy.attempts SET created_at=created_at-interval '5 hours' WHERE account_id='2'").execute(a.pool()).await.unwrap();
    release_for(&a, id(20), id(2), id(5040), "cycle", "completed").await;
    let read = s
        .autonomous_social_command_mock(id(2), "read", json!({"grant_id":id(503)}))
        .await
        .unwrap();
    let mut only_new = read["context"].clone();
    only_new["drafts"] = json!([]);
    only_new["sources"] = json!([only_new["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["event"]["id"] == json!(id(5040)))
        .unwrap()
        .clone()]);
    let mut budget_proposal =
        supa_domain::social_review::proposal(&only_new, "create", None, None, 3).unwrap();
    budget_proposal["text_sha256"] = json!(supa_providers::x_review::text_sha256(
        budget_proposal["text"].as_str().unwrap()
    ));
    let b = json!({"grant_id":id(503),"attempt_id":id(5400),"proposal":budget_proposal,"context_sha256":read["context_sha256"]});
    let first: Value = sqlx::query_scalar("SELECT social_autonomy.command($1,'reserve',$2)")
        .bind(id(2))
        .bind(b)
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(first["fresh"], true);
    s.autonomous_social_command_mock(id(2),"finish",json!({"grant_id":id(503),"attempt_id":id(5400),"receipt":{"mode":"mock","state":"failed","category":"budget_fixture_no_dispatch"}})).await.unwrap();
    sqlx::query("UPDATE social_autonomy.attempts SET created_at=clock_timestamp()-interval '5 hours' WHERE id=$1").bind(id(5400)).execute(a.pool()).await.unwrap();
    sqlx::query("UPDATE social_autonomy.grants SET next_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(id(503)).execute(a.pool()).await.unwrap();
    let read = s
        .autonomous_social_command_mock(id(2), "read", json!({"grant_id":id(503)}))
        .await
        .unwrap();
    assert_eq!(read["grant"]["reserved"], read["grant"]["budget"]);
    let err=sqlx::query_scalar::<_,Value>("SELECT social_autonomy.command($1,'reserve',$2)").bind(id(2)).bind(json!({"grant_id":id(503),"attempt_id":id(5401),"proposal":budget_proposal,"context_sha256":read["context_sha256"]})).fetch_one(s.pool()).await.unwrap_err();
    assert_eq!(
        err.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    s.autonomous_social_command_mock(id(2), "stop", json!({"grant_id":id(503)}))
        .await
        .unwrap();
    // Expired authorization cannot resume; cancellation/stop never refunds ambiguous costs.
    sqlx::query("UPDATE social_autonomy.grants SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(id(500)).execute(a.pool()).await.unwrap();
    assert!(cmd("resume").await.is_err());
    let before = wire.calls.lock().unwrap().len();
    assert_eq!(
        tick_mock(&s, id(2), id(500), id(5300), wire.clone())
            .await
            .unwrap()["state"],
        "not_due"
    );
    assert_eq!(wire.calls.lock().unwrap().len(), before);
}

#[tokio::test]
#[ignore = "explicit new disposable PG18 fixture only; fake receipts, zero X calls"]
async fn live_boundaries() {
    let a = admin().await;
    let mut agent = Agent::new(
        "private-live-fixture".into(),
        Personality::default(),
        CycleConfig::default(),
        BTreeSet::new(),
    )
    .unwrap();
    agent.id = id(30);
    a.create_owned(&agent, id(3)).await.unwrap();
    let account = json!({"id":"123","username":"supa_agency","mode":"live"});
    sqlx::query("INSERT INTO social_review.policies(agent_id,owner_id,account,enabled) VALUES($1,$2,$3,true)")
        .bind(agent.id).bind(id(3)).bind(&account).execute(a.pool()).await.unwrap();
    let s = restricted(&a).await;
    let body = json!({"grant_id":id(30000),"account":account,"confirmed":true,"scope":"original_text_released_metadata_only",
        "expires_at":(Utc::now()+Duration::hours(1)).to_rfc3339(),"max_posts":6,"interval_seconds":14400,"budget_microusd":150000,"activation_receipt_id":id(30003),"owner_proof_sha256":"a".repeat(64)});
    assert!(
        s.autonomous_social_command_mock(id(3), "authorize", body.clone())
            .await
            .is_err()
    );
    assert!(
        s.autonomous_social_command_live(id(2), "authorize", body.clone())
            .await
            .is_err()
    );
    let grant = s
        .autonomous_social_command_live(id(3), "authorize", body)
        .await
        .unwrap();
    assert_eq!(grant["state"], "paused");
    let gid = id(30000);
    let grant_body = json!({"grant_id":gid});
    assert!(
        supa_runtime::social_autonomy::live_control(&s, id(3), gid, "resume", None)
            .await
            .is_err()
    );
    // Simulate only the privileged setup's checked activation boundary. This
    // disposable SQL test never constructs a live token or root activation receipt.
    s.autonomous_social_command_live(id(3), "resume", grant_body.clone())
        .await
        .unwrap();
    let read = s
        .autonomous_social_command_live(id(3), "read", grant_body.clone())
        .await
        .unwrap();
    assert_eq!(read["due"], false); // independent private Social emergency pause
    sqlx::query("INSERT INTO social_review.pauses(agent_id,paused) VALUES($1,false)")
        .bind(agent.id)
        .execute(a.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO activity(id,agent_id,kind,code,simulated) VALUES($1,$2,'cycle','completed',true)")
        .bind(id(30001)).bind(agent.id).execute(a.pool()).await.unwrap();
    let context: Value = sqlx::query_scalar("SELECT social_review.context($1)")
        .bind(agent.id)
        .fetch_one(a.pool())
        .await
        .unwrap();
    let src = &context["sources"][0];
    sqlx::query("INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released) VALUES($1,$2,$3,$4,true)")
        .bind(agent.id).bind(id(30001)).bind(id(3)).bind(src["snapshot_sha256"].as_str().unwrap()).execute(a.pool()).await.unwrap();
    let read = s
        .autonomous_social_command_live(id(3), "read", grant_body.clone())
        .await
        .unwrap();
    assert_eq!(read["mode"], "live");
    assert_eq!(read["due"], true);
    let proposal = draft_released(&read["context"]).unwrap();
    let attempt = id(30002);
    let reserve = json!({"grant_id":gid,"attempt_id":attempt,"context_sha256":read["context_sha256"],"proposal":proposal});
    let first = s
        .autonomous_social_command_live(id(3), "reserve", reserve.clone())
        .await
        .unwrap();
    assert_eq!(first["fresh"], true);
    // A repeated proposal is now in history and fails revalidation; no second
    // reservation or dispatch is admitted.
    assert!(
        s.autonomous_social_command_live(id(3), "reserve", reserve)
            .await
            .is_err()
    );
    let guard = json!({"grant_id":gid,"attempt_id":attempt,"account":account,"text_sha256":proposal["text_sha256"]});
    assert!(
        s.autonomous_social_command_live(id(2), "guard", guard.clone())
            .await
            .is_err()
    );
    sqlx::query("INSERT INTO social_review.pauses(agent_id,paused) VALUES($1,true)")
        .bind(agent.id)
        .execute(a.pool())
        .await
        .unwrap();
    assert!(
        s.autonomous_social_command_live(id(3), "guard", guard.clone())
            .await
            .is_err()
    );
    sqlx::query("INSERT INTO social_review.pauses(agent_id,paused) VALUES($1,false)")
        .bind(agent.id)
        .execute(a.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM agent_access WHERE agent_id=$1 AND principal_id=$2")
        .bind(agent.id)
        .bind(id(3))
        .execute(a.pool())
        .await
        .unwrap();
    assert!(
        s.autonomous_social_command_live(id(3), "guard", guard.clone())
            .await
            .is_err()
    );
    sqlx::query(
        "INSERT INTO agent_access(agent_id,principal_id,role,granted_by) VALUES($1,$2,'owner',$2)",
    )
    .bind(agent.id)
    .bind(id(3))
    .execute(a.pool())
    .await
    .unwrap();
    assert_eq!(
        s.autonomous_social_command_live(id(3), "guard", guard)
            .await
            .unwrap(),
        true
    );
    // Restart recovery burns the intent and quarantines the numeric account.
    let recovered = s
        .autonomous_social_command_live(id(3), "recover", grant_body.clone())
        .await
        .unwrap();
    assert_eq!(recovered["grant"]["state"], "paused");
    assert_eq!(recovered["attempts"][0]["receipt"]["mode"], "live");
    assert_eq!(recovered["attempts"][0]["state"], "uncertain");
    assert!(
        s.autonomous_social_command_live(id(3), "resume", grant_body.clone())
            .await
            .is_err()
    );
    for op in ["pause", "stop", "revoke"] {
        supa_runtime::social_autonomy::live_control(&s, id(3), gid, op, None)
            .await
            .unwrap();
    }
    let read = s
        .autonomous_social_command_live(id(3), "read", grant_body)
        .await
        .unwrap();
    assert_eq!(read["grant"]["state"], "revoked");
    assert_eq!(read["grant"]["reserved"], 25000);
    for event in read["audit"].as_array().unwrap() {
        assert_eq!(event["body"]["mode"], "live");
    }
    let (bad,broken):(i64,i64)=sqlx::query_as("WITH e AS(SELECT *,lag(sha256,1,repeat('0',64)) OVER(PARTITION BY agent_id ORDER BY id) expected FROM social_autonomy.events) SELECT count(*) FILTER(WHERE sha256<>encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex')),count(*) FILTER(WHERE previous_sha256<>expected) FROM e").fetch_one(a.pool()).await.unwrap();
    assert_eq!((bad, broken), (0, 0));
    assert!(
        sqlx::query("UPDATE social_autonomy.grants SET reserved=0")
            .execute(s.pool())
            .await
            .is_err()
    );
}
