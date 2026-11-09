//! Complete command subprocess tests against the existing disposable socket-only PG18 fixture.
use serde_json::{Value, json};
use std::{collections::BTreeSet, process::Command};
use supa_domain::{Agent, CycleConfig, Personality};
use supa_persistence::Store;
use uuid::Uuid;
const OWNER: &str = "6fd863e1-745d-4a06-b68e-eea69e854a69";
fn agent() -> Uuid {
    Uuid::from_u128(9900)
}
async fn admin() -> Store {
    let url = std::env::var("SUPA_AUTONOMY_TEST_URL").expect("disposable PG18 fixture required");
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    assert_eq!(options.get_database(), Some("supa_autonomy_test"));
    assert_eq!(options.get_username(), "supa_autonomy_test");
    let socket = options.get_socket().unwrap();
    let parent = socket.parent().unwrap();
    assert_eq!(parent.parent(), Some(std::path::Path::new("/tmp")));
    assert!(
        parent
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("supa-autonomy-test-")
    );
    let store = Store::connect_first_post_local(&url, true).await.unwrap();
    let (listen, addr): (String, Option<String>) =
        sqlx::query_as("SELECT current_setting('listen_addresses'),inet_server_addr()::text")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(listen.is_empty());
    assert!(addr.is_none());
    store
}
fn run(args: &[String]) -> std::process::Output {
    let url = std::env::var("SUPA_AUTONOMY_TEST_URL").unwrap().replacen(
        "supa_autonomy_test@",
        "supa_first_post_runtime@",
        1,
    );
    Command::new(env!("CARGO_BIN_EXE_supa-x-first-post"))
        .args(args)
        .env_clear()
        .env("LANG", "C.UTF-8")
        .env("SUPA_X_FIRST_POST_TEST_URL", url)
        .output()
        .unwrap()
}
fn invoke(mode: &str, id: Uuid, expiry: i64) -> std::process::Output {
    run(&[
        "mock".into(),
        mode.into(),
        id.to_string(),
        expiry.to_string(),
    ])
}
fn answer(out: std::process::Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
async fn reset(s: &Store) {
    sqlx::query("DELETE FROM social_review.attempts WHERE agent_id=$1")
        .bind(agent())
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM social_review.drafts WHERE agent_id=$1")
        .bind(agent())
        .execute(s.pool())
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "explicit disposable PG18 fixture only"]
async fn journey() {
    let s = admin().await;
    sqlx::raw_sql(include_str!("../../../deployment/social-first-post.sql"))
        .execute(s.pool())
        .await
        .unwrap();
    let mut a = Agent::new(
        "synthetic first-post fixture".into(),
        Personality::default(),
        CycleConfig::default(),
        BTreeSet::new(),
    )
    .unwrap();
    a.id = agent();
    s.create_owned(&a, OWNER.parse().unwrap()).await.unwrap();
    sqlx::query("INSERT INTO social_review.policies(agent_id,owner_id,account,enabled) VALUES($1,$2,'{\"id\":\"777\",\"username\":\"supa_agency\",\"mode\":\"mock\"}',true)").bind(agent()).bind(OWNER.parse::<Uuid>().unwrap()).execute(s.pool()).await.unwrap();
    sqlx::query("INSERT INTO social_review.pauses(agent_id,paused) VALUES($1,false)")
        .bind(agent())
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::raw_sql("CREATE ROLE supa_first_post_runtime LOGIN; GRANT USAGE ON SCHEMA social_review TO supa_first_post_runtime; GRANT EXECUTE ON FUNCTION social_review.first_post(uuid,text,jsonb) TO supa_first_post_runtime;").execute(s.pool()).await.unwrap();
    let url = std::env::var("SUPA_AUTONOMY_TEST_URL").unwrap().replacen(
        "supa_autonomy_test@",
        "supa_first_post_runtime@",
        1,
    );
    let restricted = Store::connect_first_post_local(&url, true).await.unwrap();
    assert!(
        sqlx::query("SELECT * FROM social_review.drafts")
            .fetch_all(restricted.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE agent_access SET role='owner'")
            .execute(restricted.pool())
            .await
            .is_err()
    );
    for (mode, state, calls, http) in [
        ("ok", "posted", 2, Some(201)),
        ("credits", "failed", 2, Some(402)),
        ("permission", "failed", 2, Some(403)),
        ("rate", "failed", 2, Some(429)),
        ("account", "failed", 1, None),
        ("cancel", "failed", 1, None),
        ("disconnect", "uncertain", 2, None),
    ] {
        reset(&s).await;
        let id = Uuid::new_v4();
        let expiry = chrono::Utc::now().timestamp() + 300;
        let r = answer(invoke(mode, id, expiry));
        assert_eq!(r["state"], state, "{mode}");
        assert_eq!(r["mock_calls"], calls, "{mode}");
        assert_eq!(r["http_status"], json!(http), "{mode}");
        assert_eq!(r["autonomous_publishing_enabled"], false);
        if mode != "cancel" {
            let repeated = answer(invoke(mode, id, expiry));
            assert_eq!(repeated["state"], "already_reserved_no_retry");
            assert_eq!(repeated["external_requests"], 0);
        }
        assert!(
            !invoke("ok", Uuid::new_v4(), expiry).status.success(),
            "duplicate/quarantine must block new authorization"
        );
        let stored: Value =
            sqlx::query_scalar("SELECT receipt FROM social_review.drafts WHERE id=$1")
                .bind(id)
                .fetch_one(s.pool())
                .await
                .unwrap();
        assert_eq!(stored["state"], state);
        let replay = restricted
            .first_post_command(
                OWNER.parse().unwrap(),
                "settle",
                json!({"authorization_id":id,"receipt":stored}),
            )
            .await
            .unwrap();
        assert_eq!(replay, stored, "settlement is idempotent");
        if mode == "rate" {
            assert!(stored["rate_limit_reset"].as_i64().unwrap() > chrono::Utc::now().timestamp());
        }
    }
    reset(&s).await;
    let race_id = Uuid::new_v4();
    let race_expiry = chrono::Utc::now().timestamp() + 300;
    let one = std::thread::spawn(move || invoke("ok", race_id, race_expiry));
    let two = std::thread::spawn(move || invoke("ok", race_id, race_expiry));
    let answers = [answer(one.join().unwrap()), answer(two.join().unwrap())];
    assert_eq!(answers.iter().filter(|r| r["state"] == "posted").count(), 1);
    assert_eq!(
        answers
            .iter()
            .filter(|r| r["state"] == "already_reserved_no_retry")
            .count(),
        1
    );
    reset(&s).await;
    assert!(
        !invoke(
            "unauthorized",
            Uuid::new_v4(),
            chrono::Utc::now().timestamp() + 300
        )
        .status
        .success()
    );
    for expiry in [
        chrono::Utc::now().timestamp() - 1,
        chrono::Utc::now().timestamp() + 601,
    ] {
        assert!(!invoke("ok", Uuid::new_v4(), expiry).status.success());
    }
    let id = Uuid::new_v4();
    let expiry = chrono::Utc::now().timestamp() + 300;
    assert_eq!(
        answer(invoke("crash", id, expiry))["state"],
        "reserved_interruption_fixture"
    );
    let body = json!({"authorization_id":id,"text_sha256":"e8bc2bee94696da203aad809d8a80aeb05a4677960ef34d9b0b68b82d47f2dd7","account":{"id":"777","username":"supa_agency","mode":"mock"}});
    assert!(
        restricted
            .first_post_command(Uuid::from_u128(2), "peek", body.clone())
            .await
            .is_err()
    );
    assert_eq!(
        restricted
            .first_post_command(OWNER.parse().unwrap(), "peek", body.clone())
            .await
            .unwrap(),
        true
    );
    assert_eq!(
        restricted
            .first_post_command(OWNER.parse().unwrap(), "peek", body.clone())
            .await
            .unwrap(),
        true,
        "peek cannot consume claim"
    );
    assert_eq!(
        restricted
            .first_post_command(OWNER.parse().unwrap(), "claim", body.clone())
            .await
            .unwrap(),
        true
    );
    assert!(
        restricted
            .first_post_command(OWNER.parse().unwrap(), "claim", body.clone())
            .await
            .is_err(),
        "claim is one-use"
    );
    // Approval fields cannot be weakened on replay; posted settlement cannot be forged.
    let approved: Value =
        sqlx::query_scalar("SELECT approval FROM social_review.drafts WHERE id=$1")
            .bind(id)
            .fetch_one(s.pool())
            .await
            .unwrap();
    for field in ["read_write_confirmed", "credits_confirmed", "confirmed"] {
        let mut bad = approved.clone();
        bad[field] = json!(false);
        assert!(
            restricted
                .first_post_command(OWNER.parse().unwrap(), "reserve", bad)
                .await
                .is_err()
        );
    }
    let mut bad = approved.clone();
    bad["budget_microusd"] = json!(15000);
    assert!(
        restricted
            .first_post_command(OWNER.parse().unwrap(), "reserve", bad)
            .await
            .is_err()
    );
    assert!(restricted.first_post_command(OWNER.parse().unwrap(),"settle",json!({"authorization_id":id,"receipt":{"mode":"mock","state":"posted","category":"bad","http_status":201}})).await.is_err());
    // Expired authorization cannot claim a reserved post.
    reset(&s).await;
    let expired = Uuid::new_v4();
    answer(invoke("crash", expired, expiry));
    sqlx::query("UPDATE social_review.drafts SET approval=jsonb_set(approval,'{expires_at}',to_jsonb((clock_timestamp()-interval '1 second')::text)) WHERE id=$1").bind(expired).execute(s.pool()).await.unwrap();
    assert!(restricted.first_post_command(OWNER.parse().unwrap(),"peek",json!({"authorization_id":expired,"text_sha256":body["text_sha256"],"account":body["account"]})).await.is_err());
    // Preserve one genuinely in-flight fixture for the physical PG restart below.
    reset(&s).await;
    let pending = Uuid::from_u128(9910);
    answer(invoke("crash", pending, expiry));
    restricted.first_post_command(OWNER.parse().unwrap(),"claim",json!({"authorization_id":pending,"text_sha256":body["text_sha256"],"account":body["account"]})).await.unwrap();
    let chain:bool=sqlx::query_scalar("SELECT bool_and(previous_sha256=coalesce(prev,repeat('0',64))) FROM (SELECT previous_sha256,lag(sha256) OVER(ORDER BY id) prev FROM social_autonomy.events WHERE agent_id=$1) q").bind(agent()).fetch_one(s.pool()).await.unwrap();
    assert!(chain);
}
#[tokio::test]
#[ignore = "run only after fixture PostgreSQL restart"]
async fn restart() {
    let s = admin().await;
    let id = Uuid::from_u128(9910);
    let state: String =
        sqlx::query_scalar("SELECT receipt->>'state' FROM social_review.drafts WHERE id=$1")
            .bind(id)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(state, "in_flight");
    let result = answer(run(&["mock-recover".into(), id.to_string()]));
    assert_eq!(result["state"], "uncertain");
    assert_eq!(result["external_requests"], 0);
    assert!(
        !invoke("ok", Uuid::new_v4(), chrono::Utc::now().timestamp() + 300)
            .status
            .success()
    );
    let result = answer(run(&["mock-recover".into(), id.to_string()]));
    assert_eq!(result["state"], "uncertain");
    let state: String =
        sqlx::query_scalar("SELECT state FROM social_review.attempts WHERE draft_id=$1")
            .bind(id)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(state, "uncertain");
}

#[test]
fn live_command_never_accepts_noninteractive_or_malformed_authorization() {
    for args in [
        vec!["post", "synthetic-sensitive-argument"],
        vec!["post", "00000000-0000-4000-8000-000000000001", "2000000000"],
        vec![
            "post",
            "00000000-0000-4000-8000-000000000001",
            "0",
            "--authorize-one-get-and-one-post-budget-025",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_supa-x-first-post"))
            .args(args)
            .env_clear()
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-sensitive-argument"));
    }
    let expiry = (chrono::Utc::now().timestamp() + 300).to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_supa-x-first-post"))
        .args([
            "post",
            "00000000-0000-4000-8000-000000000001",
            &expiry,
            "--authorize-one-get-and-one-post-budget-025",
        ])
        .env_clear()
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let output = Command::new(env!("CARGO_BIN_EXE_supa-x-first-post"))
        .args([
            "mock",
            "ok",
            "00000000-0000-4000-8000-000000000001",
            &expiry,
        ])
        .env_clear()
        .env(
            "SUPA_X_FIRST_POST_TEST_URL",
            "postgresql://synthetic@remote.invalid/supa_autonomy_test",
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
