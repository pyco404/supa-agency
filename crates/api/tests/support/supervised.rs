//! Shares the guarded disposable factory fixture. No live hosts or credentials.
use super::*;
fn supervised_app(s: Store) -> Router {
    let rt = Runtime::new(s.clone(), Arc::new(Offline));
    router(
        AppState::new(s, rt, SERVICE.into())
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
            ])
            .unwrap()
            .with_synthetic_factory()
            .unwrap()
            .with_supervised_tasks()
            .unwrap(),
    )
}
fn input() -> Value {
    json!({"task":"Summarize a synthetic report","desired_outcome":"reviewed","material":"Synthetic report reviewed. Two sample checks passed.","synthetic_only":true,"retain_memory":true})
}
fn cmd(t: &Value, op: &str, n: u128) -> Value {
    json!({"request_id":id(n),"operation":op,"expected_revision":t["revision"],"input_sha256":t["input_sha256"],"confirmed":true})
}
async fn command(a: &Router, t: &Value, op: &str, n: u128) -> Value {
    let (status, v) = http(
        a,
        Some(OWNER),
        "POST",
        &format!("/factory/tasks/{}/command", t["id"].as_str().unwrap()),
        cmd(t, op, n),
    )
    .await;
    assert_eq!(status, 200, "operation {op}: {v}");
    v
}
pub async fn exercise() {
    let admin = admin().await;
    let s = runtime_store().await;
    sqlx::raw_sql(include_str!("../../../../deployment/agent-supervised.sql"))
        .execute(admin.pool())
        .await
        .unwrap();
    sqlx::raw_sql("GRANT USAGE ON SCHEMA agent_supervised TO supa_factory_runtime; GRANT EXECUTE ON FUNCTION agent_supervised.command(uuid,text,jsonb) TO supa_factory_runtime;").execute(admin.pool()).await.unwrap();
    s.verify_supervised_schema().await.unwrap();
    let a = supervised_app(s.clone());
    let mut config = request(800);
    config["requested_capabilities"] = json!(["read_memory", "remember", "evaluate_goal"]);
    let first = http(&a, Some(OWNER), "POST", "/factory/agents", config.clone()).await;
    assert_eq!(first.0, 200);
    let agent = first.1["agent_id"].as_str().unwrap();
    config["request_id"] = json!(id(801));
    let second = http(&a, Some(OTHER), "POST", "/factory/agents", config).await;
    assert_eq!(second.0, 200);
    let path = format!("/factory/agents/{agent}/tasks");
    let body = json!({"request_id":id(900),"input":input()});
    for (token, expected) in [
        (None, 401),
        (Some(VIEWER), 403),
        (Some(SERVICE), 403),
        (Some(OTHER), 403),
    ] {
        assert_eq!(
            http(&a, token, "POST", &path, body.clone()).await.0,
            expected
        );
    }
    assert_eq!(
        http(
            &app(s.clone(), true),
            Some(OWNER),
            "POST",
            &path,
            body.clone()
        )
        .await
        .0,
        403,
        "default off"
    );
    let (left, right) = tokio::join!(
        http(&a, Some(OWNER), "POST", &path, body.clone()),
        http(&a, Some(OWNER), "POST", &path, body.clone())
    );
    assert_eq!(left.0, 200);
    assert_eq!(left, right);
    let mut t = left.1;
    let task_path = format!("/factory/tasks/{}", t["id"].as_str().unwrap());
    assert!(
        matches!(
            s.supervised(
                id(1),
                "approve",
                json!({"task_id":t["id"],"request_id":Uuid::new_v4(),"confirmed":true})
            )
            .await,
            Err(supa_domain::Error::Stale)
        ),
        "database rejects omitted review fields independently of HTTP validation"
    );

    assert_eq!(
        http(&a, Some(OTHER), "GET", &task_path, Value::Null)
            .await
            .0,
        403
    );
    let mut invalid = cmd(&t, "approve", 901);
    invalid["confirmed"] = json!(false);
    assert_eq!(
        http(
            &a,
            Some(OWNER),
            "POST",
            &(task_path.clone() + "/command"),
            invalid
        )
        .await
        .0,
        400
    );
    assert_eq!(
        http(
            &a,
            Some(OWNER),
            "POST",
            &(task_path.clone() + "/command"),
            cmd(&t, "advance", 901)
        )
        .await
        .0,
        403
    );
    t = command(&a, &t, "approve", 902).await;
    t = command(&a, &t, "start", 903).await;
    let one = cmd(&t, "advance", 904);
    t = command(&a, &t, "advance", 904).await;
    assert_eq!(t["step"], 1);
    assert_eq!(
        http(
            &a,
            Some(OWNER),
            "POST",
            &(task_path.clone() + "/command"),
            one.clone()
        )
        .await
        .1,
        t,
        "replay did not run a second step"
    );
    let mut forged = one;
    forged["payload"] = json!({"result":"invented"});
    assert_eq!(
        http(
            &a,
            Some(OWNER),
            "POST",
            &(task_path.clone() + "/command"),
            forged
        )
        .await
        .0,
        400
    );
    let original_deadline = t["deadline"].clone();
    t = command(&a, &t, "pause", 905).await;
    assert_eq!(command_status(&a, &t, "advance", 906).await, 403);
    t = command(&a, &t, "resume", 907).await;
    assert_eq!(
        t["deadline"], original_deadline,
        "resume cannot extend execution"
    );
    // A concurrent observation/commit cannot bypass revision CAS. Runtime gate is per request.
    let (x, y) = tokio::join!(
        command_pair(&a, &t, "advance", 908),
        command_pair(&a, &t, "advance", 909)
    );
    assert!([x.0, y.0].contains(&200));
    assert!([x.0, y.0].iter().any(|x| [409, 429, 503].contains(x)));
    t = if x.0 == 200 { x.1 } else { y.1 };
    assert_eq!(t["step"], 2);
    // Revoke owner, verify reads and execution fail, then restore only this disposable ACL.
    let aid: Uuid = agent.parse().unwrap();
    sqlx::query("DELETE FROM agent_access WHERE agent_id=$1 AND principal_id=$2")
        .bind(aid)
        .bind(id(1))
        .execute(admin.pool())
        .await
        .unwrap();
    assert_eq!(
        http(&a, Some(OWNER), "GET", &task_path, Value::Null)
            .await
            .0,
        403
    );
    assert_eq!(command_status(&a, &t, "advance", 910).await, 403);
    sqlx::query(
        "INSERT INTO agent_access(agent_id,principal_id,role,granted_by) VALUES($1,$2,'owner',$2)",
    )
    .bind(aid)
    .bind(id(1))
    .execute(admin.pool())
    .await
    .unwrap();
    t = command(&a, &t, "advance", 911).await;
    assert_eq!(t["state"], "completed");
    assert_eq!(t["step"], 3);
    assert_eq!(t["payloads"][1]["report"]["status"], "completed");
    assert_eq!(t["payloads"][2]["memory"]["kind"], "hypothesis");
    assert_eq!(command_status(&a, &t, "advance", 912).await, 403);
    assert_eq!(
        admin.get(aid).await.unwrap().lifecycle,
        supa_domain::Lifecycle::Paused
    );
    assert!(admin.get(aid).await.unwrap().capabilities.is_empty());
    // Audit hashes are database-enforced; runtime cannot modify stored events or tasks.
    for sql in [
        "UPDATE agent_supervised.tasks SET state='completed'",
        "DELETE FROM agent_supervised.events",
        "DELETE FROM agent_supervised.commands",
        "UPDATE agents SET data='{}'",
    ] {
        assert!(sqlx::raw_sql(sql).execute(s.pool()).await.is_err());
    }
    let valid:bool=sqlx::query_scalar("SELECT bool_and(sha256=encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex')) FROM agent_supervised.events").fetch_one(admin.pool()).await.unwrap();
    assert!(valid);
    let linked:bool=sqlx::query_scalar("SELECT bool_and(previous_sha256=coalesce(previous,repeat('0',64))) FROM (SELECT previous_sha256,lag(sha256) OVER(PARTITION BY task_id ORDER BY revision) previous FROM agent_supervised.events) q").fetch_one(admin.pool()).await.unwrap();
    assert!(linked);
    let source: Uuid =
        serde_json::from_value(t["payloads"][2]["memory"]["source_event_id"].clone()).unwrap();
    let source_count:i64=sqlx::query_scalar("SELECT count(*) FROM agent_supervised.events WHERE id=$1 AND task_id=$2 AND body->>'step'='2'").bind(source).bind(t["id"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(admin.pool()).await.unwrap();
    assert_eq!(source_count, 1);
    let mut follow = http(
        &a,
        Some(OWNER),
        "POST",
        &path,
        json!({"request_id":id(915),"input":input()}),
    )
    .await
    .1;
    follow = command(&a, &follow, "approve", 916).await;
    follow = command(&a, &follow, "start", 917).await;
    follow = command(&a, &follow, "advance", 918).await;
    assert_eq!(
        follow["payloads"][0]["retrieved_task_memories"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "reuse existing relevance ranking on retained private memory"
    );
    command(&a, &follow, "stop", 919).await;
    let agents = http(&a, Some(OWNER), "GET", "/factory/agents", Value::Null)
        .await
        .1;
    let without = agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["configuration"]["request_id"] == id(12).to_string())
        .unwrap();
    assert_eq!(
        http(
            &a,
            Some(OWNER),
            "POST",
            &format!(
                "/factory/agents/{}/tasks",
                without["agent_id"].as_str().unwrap()
            ),
            json!({"request_id":id(914),"input":input()})
        )
        .await
        .0,
        403,
        "requested memory permission required"
    );
    // Stop and timeout are durable, terminal, and do not retain incomplete memories.
    for (n, timeout) in [(920, false), (930, true)] {
        let mut task = http(
            &a,
            Some(OWNER),
            "POST",
            &path,
            json!({"request_id":id(n),"input":input()}),
        )
        .await
        .1;
        task = command(&a, &task, "approve", n + 1).await;
        task = command(&a, &task, "start", n + 2).await;
        if timeout {
            sqlx::query("UPDATE agent_supervised.tasks SET deadline=clock_timestamp()-interval '1 second' WHERE id=$1").bind(task["id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(admin.pool()).await.unwrap();
            task = command(&a, &task, "advance", n + 3).await;
            assert_eq!(task["state"], "failed");
            assert_eq!(task["error_code"], "execution_deadline_exceeded");
        } else {
            task = command(&a, &task, "stop", n + 3).await;
            assert_eq!(task["state"], "stopped");
        }
        assert_eq!(task["step"], 0);
        assert_eq!(command_status(&a, &task, "resume", n + 4).await, 403);
    }
    // Expired approvals, forged reviews, non-synthetic input and secret-shaped material fail closed.
    let mut waiting = http(
        &a,
        Some(OWNER),
        "POST",
        &path,
        json!({"request_id":id(940),"input":input()}),
    )
    .await
    .1;
    let mut bad = cmd(&waiting, "approve", 941);
    bad["input_sha256"] = json!("0".repeat(64));
    assert_eq!(
        http(
            &a,
            Some(OWNER),
            "POST",
            &format!("/factory/tasks/{}/command", waiting["id"].as_str().unwrap()),
            bad
        )
        .await
        .0,
        409
    );
    waiting = command(&a, &waiting, "approve", 942).await;
    sqlx::query("UPDATE agent_supervised.tasks SET approved_until=clock_timestamp()-interval '1 second' WHERE id=$1").bind(waiting["id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(admin.pool()).await.unwrap();
    assert_eq!(command_status(&a, &waiting, "start", 943).await, 403);
    command(&a, &waiting, "stop", 944).await;
    for (field, value) in [
        ("synthetic_only", json!(false)),
        ("material", json!("password=never-an-approved-input")),
        ("material", json!("x".repeat(4001))),
    ] {
        let mut v = input();
        v[field] = value;
        assert!(
            [400, 403].contains(
                &http(
                    &a,
                    Some(OWNER),
                    "POST",
                    &path,
                    json!({"request_id":Uuid::new_v4(),"input":v})
                )
                .await
                .0
            )
        );
    }
    // A trigger failure rolls back the step, event and idempotency marker together.
    let mut task = http(
        &a,
        Some(OWNER),
        "POST",
        &path,
        json!({"request_id":id(950),"input":input()}),
    )
    .await
    .1;
    task = command(&a, &task, "approve", 951).await;
    task = command(&a, &task, "start", 952).await;
    sqlx::raw_sql("CREATE FUNCTION agent_supervised.fail_event() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic_failure'; END $$;CREATE TRIGGER fail_event BEFORE INSERT ON agent_supervised.events FOR EACH STATEMENT EXECUTE FUNCTION agent_supervised.fail_event();").execute(admin.pool()).await.unwrap();
    assert_eq!(command_status(&a, &task, "advance", 953).await, 503);
    assert_eq!(
        http(
            &a,
            Some(OWNER),
            "GET",
            &format!("/factory/tasks/{}", task["id"].as_str().unwrap()),
            Value::Null
        )
        .await
        .1["step"],
        0
    );
    sqlx::raw_sql("DROP TRIGGER fail_event ON agent_supervised.events;DROP FUNCTION agent_supervised.fail_event();").execute(admin.pool()).await.unwrap();
    // Leave this running at step zero: restart must not auto-run or consume another step.
    if std::env::var("SUPA_FACTORY_BROWSER").as_deref() == Ok("1") {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:3352")
            .await
            .unwrap();
        let svc = a.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, svc).await.unwrap();
        });
        let result = tokio::task::spawn_blocking(|| {
            std::process::Command::new("/home/pikoo/.nvm/versions/node/v24.22.1/bin/node")
                .arg("scripts/test-supervised-journey.mjs")
                .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
                .env("SUPA_FACTORY_BROWSER_TOKEN", OWNER)
                .env("SUPA_FACTORY_OTHER_TOKEN", OTHER)
                .status()
                .unwrap()
        })
        .await
        .unwrap();
        server.abort();
        assert!(result.success(), "supervised_browser_failed");
    }
    println!(
        "supervised deterministic task completed; synthetic-only; no spending or external calls"
    );
}
async fn command_pair(a: &Router, t: &Value, op: &str, n: u128) -> (u16, Value) {
    http(
        a,
        Some(OWNER),
        "POST",
        &format!("/factory/tasks/{}/command", t["id"].as_str().unwrap()),
        cmd(t, op, n),
    )
    .await
}
async fn command_status(a: &Router, t: &Value, op: &str, n: u128) -> u16 {
    command_pair(a, t, op, n).await.0
}
pub async fn recovery() {
    let s = runtime_store().await;
    let a = supervised_app(s.clone());
    let agents = http(&a, Some(OWNER), "GET", "/factory/agents", Value::Null)
        .await
        .1;
    let agent = agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["configuration"]["request_id"] == id(800).to_string())
        .unwrap();
    let list = http(
        &a,
        Some(OWNER),
        "GET",
        &format!(
            "/factory/agents/{}/tasks",
            agent["agent_id"].as_str().unwrap()
        ),
        Value::Null,
    )
    .await;
    assert_eq!(list.0, 200);
    let tasks = list.1["tasks"].as_array().unwrap();
    assert!(
        tasks.iter().any(
            |t| t["state"] == "completed" && t["payloads"][2]["memory"]["kind"] == "hypothesis"
        )
    );
    let running = tasks
        .iter()
        .find(|t| t["input"]["task"] == "Summarize a synthetic report" && t["state"] == "running")
        .unwrap();
    assert_eq!(running["step"], 0, "restart never autoexecutes");
    assert_eq!(
        http(
            &a,
            Some(OTHER),
            "GET",
            &format!("/factory/tasks/{}", running["id"].as_str().unwrap()),
            Value::Null
        )
        .await
        .0,
        403
    );
    println!("supervised PostgreSQL restart persistence and owner isolation passed");
}
