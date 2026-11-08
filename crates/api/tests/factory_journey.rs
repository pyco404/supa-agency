//! Explicit disposable PG18 only. Real auth, transactions, restricted SQL and browser.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, postgres::PgConnectOptions};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use supa_api::{AppState, OperatorCredential, router};
use supa_persistence::{FactoryRequest, Role, Store};
use supa_providers::Offline;
use supa_runtime::Runtime;
use tower::ServiceExt;
use uuid::Uuid;
#[path = "support/avatars.rs"]
mod avatar_tests;
#[path = "support/supervised.rs"]
mod supervised_tests;
const OWNER: &str = "factory-synthetic-owner-credential-00000000";
const OTHER: &str = "factory-synthetic-other-credential-00000000";
const VIEWER: &str = "factory-synthetic-viewer-credential-0000000";
const EXPIRED: &str = "factory-synthetic-expired-credential-000000";
const SERVICE: &str = "factory-synthetic-service-credential-000000";
fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x11111111111141118000000000000000 | n)
}
fn permitted(o: &PgConnectOptions) -> bool {
    o.get_database() == Some("supa_factory_test")
        && o.get_username() == "supa_factory_test"
        && o.get_port() == 55481
        && o.get_socket().is_some_and(|s| {
            s.file_name().is_some_and(|n| n == "socket")
                && s.parent().is_some_and(|p| {
                    p.parent() == Some(Path::new("/tmp"))
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .and_then(|n| n.strip_prefix("supa-factory-test-"))
                            .is_some_and(|n| {
                                n.len() == 32 && n.bytes().all(|b| b.is_ascii_hexdigit())
                            })
                })
        })
}
async fn admin() -> Store {
    let o: PgConnectOptions = std::env::var("SUPA_FACTORY_TEST_URL")
        .expect("explicit_disposable_url_required")
        .parse()
        .unwrap();
    assert!(permitted(&o));
    assert_eq!(
        o.get_socket().unwrap().canonicalize().unwrap(),
        o.get_socket().unwrap().to_path_buf()
    );
    let s = Store::connect(o.to_url_lossy().as_ref()).await.unwrap();
    let (version,db,user,listen,address):(String,String,String,String,Option<String>)=sqlx::query_as("SELECT version(),current_database(),current_user::text,current_setting('listen_addresses'),inet_server_addr()::text").fetch_one(s.pool()).await.unwrap();
    assert!(version.contains("PostgreSQL 18."));
    assert_eq!(db, "supa_factory_test");
    assert_eq!(user, "supa_factory_test");
    assert!(listen.is_empty() && address.is_none());
    s
}
async fn runtime_store() -> Store {
    let o: PgConnectOptions = std::env::var("SUPA_FACTORY_TEST_URL")
        .unwrap()
        .parse()
        .unwrap();
    assert!(permitted(&o));
    Store::connect(o.username("supa_factory_runtime").to_url_lossy().as_ref())
        .await
        .unwrap()
}
fn app(s: Store, enabled: bool) -> Router {
    let rt = Runtime::new(s.clone(), Arc::new(Offline));
    let mut state = AppState::new(s, rt, SERVICE.into())
        .unwrap()
        .with_credentials(vec![
            OperatorCredential {
                id: id(1),
                role: Role::Owner,
                token: OWNER.into(),
            },
            OperatorCredential {
                id: id(2),
                role: Role::Owner,
                token: OTHER.into(),
            },
            OperatorCredential {
                id: id(3),
                role: Role::Viewer,
                token: VIEWER.into(),
            },
            OperatorCredential {
                id: id(4),
                role: Role::Owner,
                token: EXPIRED.into(),
            },
        ])
        .unwrap()
        .with_credential_deadlines(BTreeMap::from([(id(4), 1)]))
        .unwrap();
    if enabled {
        state = state.with_synthetic_factory().unwrap();
        if std::env::var("SUPA_FACTORY_AVATARS").as_deref() == Ok("1") {
            state = state.with_agent_avatars().unwrap();
        }
    }
    router(state)
}
fn request(n: u128) -> Value {
    json!({"request_id":id(n),"name":"Synthetic research assistant","description":"Disposable demonstration only","purpose":"Summarize synthetic reports for review","personality":{"voice":"Curious, concise and dryly witty","traits":["curious"]},"goals":["Summarize a synthetic report"],"requested_capabilities":["read_memory","evaluate_goal"],"autonomy":"disabled","spending_limit_microusd":0,"private_consent":true})
}
async fn http(
    a: &Router,
    token: Option<&str>,
    method: &str,
    path: &str,
    body: Value,
) -> (u16, Value) {
    let mut r = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(t) = token {
        r = r.header("authorization", format!("Bearer {t}"));
    }
    let response = a
        .clone()
        .oneshot(
            r.body(if method == "GET" {
                Body::empty()
            } else {
                Body::from(body.to_string())
            })
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1_500_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[test]
fn refuses_unscoped_database_targets() {
    for u in [
        "postgresql://localhost/supa_staging",
        "postgresql://supa_factory_test@localhost/supa_factory_test?port=55481",
        "postgresql://localhost/postgres?host=/tmp/socket",
    ] {
        assert!(!permitted(&u.parse().unwrap()));
    }
}
#[tokio::test]
#[ignore = "requires explicit disposable PG18 runner"]
async fn factory_journey() {
    let admin = admin().await;
    admin.migrate().await.unwrap();
    sqlx::raw_sql(include_str!("../../../deployment/ecosystem-social.sql"))
        .execute(admin.pool())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../../../deployment/agent-factory.sql"))
        .execute(admin.pool())
        .await
        .unwrap();
    sqlx::raw_sql("CREATE ROLE supa_factory_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; REVOKE CREATE ON SCHEMA public FROM PUBLIC; GRANT USAGE ON SCHEMA public,agent_factory TO supa_factory_runtime; GRANT SELECT ON application_epoch,agent_access,_sqlx_migrations TO supa_factory_runtime; GRANT EXECUTE ON FUNCTION agent_factory.create_private(uuid,uuid,jsonb,jsonb,jsonb,jsonb),agent_factory.owned(uuid),agent_factory.managed(uuid) TO supa_factory_runtime;").execute(admin.pool()).await.unwrap();
    if std::env::var("SUPA_FACTORY_AVATARS").as_deref() == Ok("1") {
        sqlx::raw_sql(include_str!("../../../deployment/agent-avatars.sql"))
            .execute(admin.pool())
            .await
            .unwrap();
        sqlx::raw_sql("GRANT USAGE ON SCHEMA agent_avatars TO supa_factory_runtime; GRANT EXECUTE ON FUNCTION agent_avatars.command(uuid,text,jsonb),agent_avatars.create_agent(uuid,uuid,jsonb,jsonb,jsonb,jsonb,uuid) TO supa_factory_runtime;").execute(admin.pool()).await.unwrap();
    }
    let s = runtime_store().await;
    let a = app(s.clone(), true);
    for (token, code) in [
        (None, 401),
        (Some(SERVICE), 403),
        (Some(VIEWER), 403),
        (Some(EXPIRED), 401),
    ] {
        assert_eq!(
            http(&a, token, "POST", "/factory/agents", request(10))
                .await
                .0,
            code
        );
    }
    assert_eq!(
        http(
            &app(s.clone(), false),
            Some(OWNER),
            "POST",
            "/factory/agents",
            request(10)
        )
        .await
        .0,
        403
    );
    for (field, value) in [
        ("autonomy", json!("active")),
        ("spending_limit_microusd", json!(1)),
        ("private_consent", json!(false)),
        ("creator_id", json!(id(2))),
        ("purpose", json!("")),
        ("goals", json!(["duplicate", "duplicate"])),
        ("token_mint", json!("forged")),
    ] {
        let mut b = request(10);
        b[field] = value;
        assert_eq!(
            http(&a, Some(OWNER), "POST", "/factory/agents", b).await.0,
            400
        );
    }
    let (first, second) = tokio::join!(
        http(&a, Some(OWNER), "POST", "/factory/agents", request(10)),
        http(&a, Some(OWNER), "POST", "/factory/agents", request(10))
    );
    assert_eq!(first.0, 200);
    assert_eq!(first, second);
    let agent = Uuid::parse_str(first.1["agent_id"].as_str().unwrap()).unwrap();
    let detail = format!("/factory/agents/{agent}");
    assert_eq!(
        http(&a, Some(OWNER), "GET", &detail, Value::Null).await,
        first
    );
    for (token, code) in [
        (None, 401),
        (Some(SERVICE), 403),
        (Some(VIEWER), 403),
        (Some(EXPIRED), 401),
        (Some(OTHER), 404),
    ] {
        assert_eq!(http(&a, token, "GET", &detail, Value::Null).await.0, code);
    }
    assert_eq!(
        http(
            &a,
            Some(OWNER),
            "GET",
            &format!("/factory/agents/{}", id(9999)),
            Value::Null
        )
        .await
        .0,
        404
    );
    assert_eq!(
        http(
            &app(s.clone(), false),
            Some(OWNER),
            "GET",
            &detail,
            Value::Null
        )
        .await
        .0,
        403
    );
    let mut conflict = request(10);
    conflict["name"] = json!("Changed");
    assert_eq!(
        http(&a, Some(OWNER), "POST", "/factory/agents", conflict)
            .await
            .0,
        409
    );
    let fresh = app(runtime_store().await, true);
    assert_eq!(
        http(&fresh, Some(OWNER), "POST", "/factory/agents", request(10)).await,
        first
    );
    assert_eq!(
        http(&fresh, Some(OTHER), "GET", "/factory/agents", Value::Null)
            .await
            .1,
        json!({"agents":[]})
    );
    for endpoint in [
        "lifecycle",
        "pause",
        "resume",
        "emergency-stop",
        "goals",
        "access",
        "ecosystem-consent",
    ] {
        assert_eq!(
            http(
                &a,
                Some(OWNER),
                "POST",
                &format!("/agents/{agent}/{endpoint}"),
                json!({"state":"active","reason":"attempt"})
            )
            .await
            .0,
            403
        );
    }
    // No generic runtime schema, ACL, migration, finance or private-memory writes.
    for statement in [
        "CREATE TABLE public.factory_denied(id int)",
        "UPDATE agent_access SET role='owner'",
        "INSERT INTO access_audit(agent_id,principal_id,role,approved_by) VALUES(gen_random_uuid(),gen_random_uuid(),'owner',gen_random_uuid())",
        "UPDATE agents SET data='{}'::jsonb",
        "DELETE FROM agent_factory.submissions",
        "TRUNCATE _sqlx_migrations",
        "UPDATE simulated_ledger SET amount_microusd=1",
        "UPDATE economic_jobs SET state='completed'",
        "ALTER ROLE supa_factory_runtime SUPERUSER",
    ] {
        assert!(
            sqlx::raw_sql(statement).execute(s.pool()).await.is_err(),
            "unauthorized SQL unexpectedly accepted"
        );
    }
    assert_eq!(
        http(
            &a,
            Some(OTHER),
            "GET",
            &format!("/agents/{agent}"),
            Value::Null
        )
        .await
        .0,
        403
    );
    assert!(
        sqlx::query("SELECT agent_factory.create_private($1,$2,$3,$4,$5,$6)")
            .bind(id(1))
            .bind(id(99))
            .bind(json!({"autonomy":"active"}))
            .bind(json!({}))
            .bind(json!({}))
            .bind(json!({}))
            .execute(s.pool())
            .await
            .is_err()
    );
    let stored = admin.get(agent).await.unwrap();
    assert_eq!(stored.lifecycle, supa_domain::Lifecycle::Paused);
    assert!(
        stored.capabilities.is_empty()
            && stored.memory.is_empty()
            && !stored.config.scheduler_enabled
    );
    assert_eq!(stored.goals.len(), 1);
    let balance: i64 =
        sqlx::query_scalar("SELECT simulated_balance_microusd FROM agents WHERE id=$1")
            .bind(agent)
            .fetch_one(admin.pool())
            .await
            .unwrap();
    assert_eq!(balance, 0);
    assert!(
        sqlx::query(
            "UPDATE agents SET data=jsonb_set(data,'{lifecycle}','\"active\"') WHERE id=$1"
        )
        .bind(agent)
        .execute(admin.pool())
        .await
        .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM agent_factory.submissions")
            .fetch_one(admin.pool())
            .await
            .unwrap(),
        1
    );
    // A late database failure rolls the entire creation back.
    sqlx::raw_sql("CREATE FUNCTION agent_factory.test_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic_commit_failure'; END $$; CREATE TRIGGER factory_test_failure BEFORE INSERT ON agent_factory.submissions FOR EACH STATEMENT EXECUTE FUNCTION agent_factory.test_fail();").execute(admin.pool()).await.unwrap();
    assert_eq!(
        http(&a, Some(OWNER), "POST", "/factory/agents", request(11))
            .await
            .0,
        503
    );
    sqlx::raw_sql("DROP TRIGGER factory_test_failure ON agent_factory.submissions; DROP FUNCTION agent_factory.test_fail();").execute(admin.pool()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM agents")
            .fetch_one(admin.pool())
            .await
            .unwrap(),
        1
    );
    let (left, right) = tokio::join!(
        http(&a, Some(OWNER), "POST", "/factory/agents", request(12)),
        http(&a, Some(OWNER), "POST", "/factory/agents", request(13))
    );
    assert_eq!(left.0, 200);
    assert_eq!(right.0, 200);
    assert_ne!(left.1["agent_id"], right.1["agent_id"]);
    if std::env::var("SUPA_FACTORY_AVATARS").as_deref() == Ok("1") {
        avatar_tests::exercise(&a, &admin, &s, agent).await;
    }
    // Full browser -> Next -> Rust -> PostgreSQL path. No live URLs or credentials.
    if std::env::var("SUPA_FACTORY_BROWSER").as_deref() == Ok("1") {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:3351")
            .await
            .unwrap();
        let service = a.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        });
        let result = tokio::task::spawn_blocking(|| {
            std::process::Command::new("/home/pikoo/.nvm/versions/node/v24.22.1/bin/node")
                .arg("scripts/test-factory-journey.mjs")
                .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
                .env("SUPA_FACTORY_BROWSER_TOKEN", OWNER)
                .env("SUPA_FACTORY_VIEWER_TOKEN", VIEWER)
                .env("SUPA_FACTORY_SERVICE_TOKEN", SERVICE)
                .status()
                .unwrap()
        })
        .await
        .unwrap();
        server.abort();
        assert!(result.success(), "real_browser_journey_failed");
    }
    assert!(
        http(&a, Some(OWNER), "GET", "/factory/agents", Value::Null)
            .await
            .1["agents"]
            .as_array()
            .unwrap()
            .len()
            >= 1
    );
    if std::env::var("SUPA_FACTORY_AVATARS").as_deref() == Ok("1") {
        let body = json!({"request_id":id(500),"expected_revision":2,"public":false,"revoked":true,"valid_from":chrono::Utc::now(),"expires_at":chrono::Utc::now(),"categories":[]});
        sqlx::query("INSERT INTO ecosystem_social.consents(request_id,agent_id,creator_id,revision,body) VALUES($1,$2,$3,3,$4)").bind(id(500)).bind(agent).bind(id(1)).bind(body).execute(admin.pool()).await.unwrap();
        assert_eq!(
            http(
                &a,
                Some(SERVICE),
                "GET",
                &format!("/public/agent-avatars/{agent}/2"),
                Value::Null
            )
            .await
            .0,
            404
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM agents")
        .fetch_one(admin.pool())
        .await
        .unwrap();
    println!("synthetic_factory_persistence_count={count}");
    let _: FactoryRequest = serde_json::from_value(request(10)).unwrap();
}
#[tokio::test]
#[ignore = "requires runner PostgreSQL restart"]
async fn factory_restart_recovery() {
    let admin = admin().await;
    let s = runtime_store().await;
    let a = app(s.clone(), true);
    let before = http(&a, Some(OWNER), "GET", "/factory/agents", Value::Null).await;
    assert_eq!(before.0, 200);
    assert!(!before.1["agents"].as_array().unwrap().is_empty());
    if std::env::var("SUPA_FACTORY_AVATARS").as_deref() == Ok("1") {
        let matching:i64=sqlx::query_scalar("SELECT count(*) FROM agent_avatars.token_images t JOIN agent_avatars.assets a ON a.id=t.asset_id WHERE t.sha256=encode(sha256(a.png),'hex')").fetch_one(admin.pool()).await.unwrap();
        assert_eq!(
            matching, 2,
            "both immutable token image snapshots must survive PostgreSQL restart"
        );
        assert!(
            http(
                &a,
                Some(SERVICE),
                "GET",
                "/public/agent-profiles",
                Value::Null
            )
            .await
            .1["agents"]
                .as_array()
                .unwrap()
                .is_empty(),
            "revoked public consent must remain revoked after restart"
        );
    }
    let replay = http(&a, Some(OWNER), "POST", "/factory/agents", request(10)).await;
    assert_eq!(replay.0, 200);
    assert_eq!(
        http(&a, Some(OWNER), "GET", "/factory/agents", Value::Null).await,
        before
    );
    assert_eq!(
        http(&a, Some(OTHER), "GET", "/factory/agents", Value::Null)
            .await
            .1,
        json!({"agents":[]})
    );
    let id = Uuid::parse_str(replay.1["agent_id"].as_str().unwrap()).unwrap();
    let detail = format!("/factory/agents/{id}");
    assert_eq!(
        http(&a, Some(OWNER), "GET", &detail, Value::Null).await,
        replay
    );
    assert_eq!(
        http(&a, Some(OTHER), "GET", &detail, Value::Null).await.0,
        404
    );
    sqlx::query("DELETE FROM agent_access WHERE agent_id=$1")
        .bind(id)
        .execute(admin.pool())
        .await
        .unwrap();
    assert_eq!(
        http(&a, Some(OWNER), "GET", &detail, Value::Null).await.0,
        404
    );
    assert_eq!(
        http(&a, Some(OWNER), "POST", "/factory/agents", request(10))
            .await
            .0,
        403
    );
    assert!(
        http(&a, Some(OWNER), "GET", "/factory/agents", Value::Null)
            .await
            .1["agents"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["agent_id"] != json!(id))
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable PG18 runner after factory tests"]
async fn supervised_journey() {
    supervised_tests::exercise().await;
}
#[tokio::test]
#[ignore = "requires supervised runner PostgreSQL restart"]
async fn supervised_restart_recovery() {
    supervised_tests::recovery().await;
}
