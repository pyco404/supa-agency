//! Explicit disposable-PG18 mock reasoning integration; never contacts a model.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use supa_domain::*;
use supa_persistence::SupervisedCommand;
use supa_providers::{Invocation, controlled::ControlledProvider, supervised_mock::SupervisedMock};
struct Probe {
    calls: AtomicUsize,
    mode: &'static str,
    plan: ReasoningPlan,
}
impl Probe {
    fn new(mode: &'static str) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            mode,
            plan: if mode.starts_with("real_") {
                real_plan()
            } else {
                ReasoningPlan::mock()
            },
        }
    }
}
#[async_trait::async_trait]
impl ControlledProvider for Probe {
    fn policy(&self) -> InferencePolicy {
        self.plan.policy().unwrap()
    }
    fn project(&self) -> &str {
        if self.plan.real.is_some() {
            "proj_fixture"
        } else {
            "local-synthetic"
        }
    }
    fn work_evidence_kind(&self) -> &str {
        &self.plan.provider
    }
    async fn verify(&self) -> Result<()> {
        Ok(())
    }
    async fn reason(&self, _: &str, _: &[Memory]) -> Invocation<LiveProposal> {
        panic!("generic reasoning forbidden")
    }
    async fn work(&self, input: &LiveWorkInput) -> Invocation<WorkProposal> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if matches!(self.mode, "slow" | "timeout" | "real_slow") {
            tokio::time::sleep(std::time::Duration::from_millis(
                if matches!(self.mode, "slow" | "real_slow") {
                    500
                } else {
                    2500
                },
            ))
            .await;
        }
        if self.mode == "real_rejected" {
            let mut diagnostic = ProviderDiagnostic::new(
                ProviderOperation::Generation,
                ProviderStage::OutputValidation,
            );
            diagnostic.dispatch_intended = true;
            diagnostic.response_complete = true;
            diagnostic.headers_received(200);
            diagnostic.request_id = Some("req_fixture123".into());
            diagnostic.response_id = Some("resp_fixture123".into());
            diagnostic.reported_model = Some("gpt-6-luna".into());
            diagnostic.response_status = Some("incomplete".into());
            diagnostic.output_category = Some("max_output_tokens".into());
            diagnostic.failure = Some(ProviderFailure::InvalidOutput);
            return Invocation {
                result: Err(Error::Provider),
                usage: Some(TokenUsage {
                    input_tokens: 300,
                    output_tokens: 256,
                    ..Default::default()
                }),
                diagnostic: Some(diagnostic),
            };
        }
        if self.mode == "real_transport" {
            return Invocation {
                result: Err(Error::ProviderTransport),
                usage: None,
                diagnostic: None,
            };
        }
        let mut r = SupervisedMock.work(input).await;
        if self.plan.real.is_some() {
            r.usage = Some(TokenUsage {
                input_tokens: 300,
                output_tokens: 100,
                ..Default::default()
            });
        }
        if self.mode == "bad_claim" {
            r.result.as_mut().unwrap().findings[0].observation =
                "Invented incident diagnosis".into();
            r.usage.as_mut().unwrap().output_tokens =
                mock_reasoning_tokens(r.result.as_ref().unwrap()).unwrap();
        }
        if self.mode == "over_limit" {
            r.usage.as_mut().unwrap().output_tokens = 257;
        }
        if self.mode == "missing_usage" {
            r.usage = None;
        }
        r
    }
}
fn input() -> Value {
    json!({"task":"Review synthetic incident","desired_outcome":"recovered","material":"12:00 Synthetic requests timed out.\n12:05 Synthetic service recovered after restart.","synthetic_only":true,"retain_memory":true,"reasoning":ReasoningPlan::mock()})
}
fn command(t: &Value, op: &str) -> Value {
    json!({"request_id":Uuid::new_v4(),"operation":op,"expected_revision":t["revision"],"input_sha256":t["input_sha256"],"confirmed":true})
}
fn path(t: &Value) -> String {
    format!("/factory/tasks/{}/command", t["id"].as_str().unwrap())
}
async fn cmd(a: &Router, t: &Value, op: &str) -> Value {
    let r = http(a, Some(OWNER), "POST", &path(t), command(t, op)).await;
    assert_eq!(r.0, 200, "{op}: {}", r.1);
    r.1
}
async fn ready(a: &Router, agent: &str) -> Value {
    let r = http(
        a,
        Some(OWNER),
        "POST",
        &format!("/factory/agents/{agent}/tasks"),
        json!({"request_id":Uuid::new_v4(),"input":input()}),
    )
    .await;
    assert_eq!(r.0, 200, "create: {}", r.1);
    let t = cmd(a, &r.1, "approve").await;
    let t = cmd(a, &t, "start").await;
    cmd(a, &t, "advance").await
}
async fn get(s: &Store, t: &Value) -> Value {
    s.supervised(id(1), "get", json!({"task_id":t["id"]}))
        .await
        .unwrap()
}
async fn wait_reserved(s: &Store, t: &Value) -> Value {
    for _ in 0..100 {
        let current = get(s, t).await;
        if current["inference"].is_object() {
            return current;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("reservation deadline")
}
async fn wait_dispatched(provider: &Probe) {
    for _ in 0..100 {
        if provider.calls.load(Ordering::SeqCst) == 1 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("provider dispatch deadline");
}
pub async fn exercise(a: &Router, s: &Store, admin: &Store) {
    let financial_before: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM simulated_ledger),(SELECT count(*) FROM inference_calls)",
    )
    .fetch_one(admin.pool())
    .await
    .unwrap();
    let mut config = request(2000);
    config["requested_capabilities"] = json!(["read_memory", "remember", "evaluate_goal"]);
    let r = http(a, Some(OWNER), "POST", "/factory/agents", config.clone()).await;
    assert_eq!(r.0, 200);
    let agent = r.1["agent_id"].as_str().unwrap();
    config["request_id"] = json!(id(2001));
    let other = http(a, Some(OTHER), "POST", "/factory/agents", config).await;
    assert_eq!(other.0, 200);
    let create_path = format!("/factory/agents/{agent}/tasks");
    let body = json!({"request_id":id(2010),"input":input()});
    for (who, status) in [
        (None, 401),
        (Some(OTHER), 403),
        (Some(VIEWER), 403),
        (Some(SERVICE), 403),
    ] {
        assert_eq!(
            http(a, who, "POST", &create_path, body.clone()).await.0,
            status
        );
    }
    let (x, y) = tokio::join!(
        http(a, Some(OWNER), "POST", &create_path, body.clone()),
        http(a, Some(OWNER), "POST", &create_path, body.clone())
    );
    assert_eq!(x.0, 200);
    assert_eq!(y.0, 200);
    assert_eq!(x.1["id"], y.1["id"]);
    let mut changed = body.clone();
    changed["input"]["material"] = json!("Different synthetic input");
    assert_eq!(
        http(a, Some(OWNER), "POST", &create_path, changed).await.0,
        409
    );
    let mut bad = command(&x.1, "approve");
    bad["input_sha256"] = json!("0".repeat(64));
    assert_eq!(http(a, Some(OWNER), "POST", &path(&x.1), bad).await.0, 409);
    cmd(a, &x.1, "stop").await;
    for field in [
        "model",
        "max_requests",
        "max_output_tokens",
        "max_input_tokens",
        "max_cost_microusd",
        "provider",
        "external_tools",
    ] {
        let mut invalid = input();
        invalid["reasoning"][field] = match field {
            "model" | "provider" => json!("unapproved"),
            "external_tools" => json!(["web"]),
            _ => json!(9999),
        };
        let r = http(
            a,
            Some(OWNER),
            "POST",
            &create_path,
            json!({"request_id":Uuid::new_v4(),"input":invalid}),
        )
        .await;
        assert!([400, 403].contains(&r.0), "invalid plan {field}");
    }
    let rt = Runtime::new(s.clone(), Arc::new(Offline));
    let t = ready(a, agent).await;
    let provider = Probe::new("success");
    let c: SupervisedCommand = serde_json::from_value(command(&t, "advance")).unwrap();
    let task = Uuid::parse_str(t["id"].as_str().unwrap()).unwrap();
    let (left, right) = tokio::join!(
        rt.supervised_reasoning_with_provider(id(1), task, &c, &provider),
        rt.supervised_reasoning_with_provider(id(1), task, &c, &provider)
    );
    assert!(left.is_ok() || right.is_ok());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let result = get(s, &t).await;
    assert_eq!(result["state"], "awaiting_review");
    assert_eq!(result["payloads"].as_array().unwrap().len(), 2);
    assert_eq!(result["inference"]["state"], "settled");
    assert_eq!(result["inference"]["actual_microusd"], 0);
    assert!(
        result["inference"]["usage"]["output_tokens"]
            .as_u64()
            .unwrap()
            <= 256
    );
    assert_eq!(
        rt.supervised_reasoning_with_provider(id(1), task, &c, &provider)
            .await
            .unwrap(),
        result
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let mut changed_replay = c.clone();
    changed_replay.confirmed = !c.confirmed;
    assert!(
        rt.supervised_reasoning_with_provider(id(1), task, &changed_replay, &provider)
            .await
            .is_err()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let mut review = command(&result, "review");
    review["result_sha256"] = json!("0".repeat(64));
    assert_eq!(
        http(a, Some(OWNER), "POST", &path(&result), review.clone())
            .await
            .0,
        403
    );
    review["result_sha256"] = result["payloads"][1]["result_sha256"].clone();
    let mut no_consent = review.clone();
    no_consent["confirmed"] = json!(false);
    assert_eq!(
        http(a, Some(OWNER), "POST", &path(&result), no_consent)
            .await
            .0,
        400
    );
    assert_eq!(
        http(a, Some(OTHER), "POST", &path(&result), review.clone())
            .await
            .0,
        403
    );
    let done = http(a, Some(OWNER), "POST", &path(&result), review.clone()).await;
    assert_eq!(done.0, 200);
    assert_eq!(done.1["state"], "completed");
    assert_eq!(done.1["payloads"][2]["memory"]["kind"], "hypothesis");
    assert_eq!(
        done.1["payloads"][2]["memory"]["source_result_sha256"],
        result["inference"]["result_sha256"]
    );
    assert_eq!(
        http(a, Some(OWNER), "POST", &path(&result), review).await.1,
        done.1
    );
    // Provider failures retain an honest receipt but no accepted output or memory.
    for mode in ["bad_claim", "over_limit", "missing_usage", "timeout"] {
        let t = ready(a, agent).await;
        let provider = Probe::new(mode);
        let c = serde_json::from_value(command(&t, "advance")).unwrap();
        let result = rt
            .supervised_reasoning_with_provider(
                id(1),
                Uuid::parse_str(t["id"].as_str().unwrap()).unwrap(),
                &c,
                &provider,
            )
            .await
            .unwrap();
        assert_eq!(result["state"], "failed", "{mode}");
        assert_eq!(result["payloads"].as_array().unwrap().len(), 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
    // Expiry prevents even one provider call.
    let t = ready(a, agent).await;
    sqlx::query("UPDATE agent_supervised.tasks SET approved_until=clock_timestamp()-interval '1 second' WHERE id=$1").bind(t["id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(admin.pool()).await.unwrap();
    let provider = Probe::new("success");
    let c = serde_json::from_value(command(&t, "advance")).unwrap();
    assert!(
        rt.supervised_reasoning_with_provider(
            id(1),
            t["id"].as_str().unwrap().parse().unwrap(),
            &c,
            &provider
        )
        .await
        .is_err()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    cmd(a, &t, "stop").await;
    // Pause during a request drops the observation, records cancellation and never reissues it.
    let t = ready(a, agent).await;
    let provider = Probe::new("slow");
    let c = serde_json::from_value(command(&t, "advance")).unwrap();
    let task = t["id"].as_str().unwrap().parse().unwrap();
    let (execution, paused) = tokio::join!(
        rt.supervised_reasoning_with_provider(id(1), task, &c, &provider),
        async {
            let current = wait_reserved(s, &t).await;
            wait_dispatched(&provider).await;
            cmd(a, &current, "pause").await
        }
    );
    assert_eq!(paused["state"], "paused");
    assert!(execution.is_ok());
    let paused = get(s, &t).await;
    assert_eq!(paused["inference"]["state"], "discarded");
    assert_eq!(paused["payloads"].as_array().unwrap().len(), 1);
    cmd(a, &paused, "stop").await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    // Stop during an in-flight request must also discard output without a second call.
    let t = ready(a, agent).await;
    let provider = Probe::new("slow");
    let c = serde_json::from_value(command(&t, "advance")).unwrap();
    let task = t["id"].as_str().unwrap().parse().unwrap();
    let (execution, stopped) = tokio::join!(
        rt.supervised_reasoning_with_provider(id(1), task, &c, &provider),
        async {
            let current = wait_reserved(s, &t).await;
            wait_dispatched(&provider).await;
            cmd(a, &current, "stop").await
        }
    );
    assert_eq!(stopped["state"], "stopped");
    assert!(execution.is_ok());
    let stopped = get(s, &t).await;
    assert_eq!(stopped["inference"]["state"], "discarded");
    assert_eq!(stopped["payloads"].as_array().unwrap().len(), 1);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    // Settlement is atomic even when a database trigger rejects the audit event.
    let t = ready(a, agent).await;
    sqlx::raw_sql("CREATE FUNCTION agent_supervised.fail_reasoning_event() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.body->>'operation'='settle_inference' THEN RAISE EXCEPTION 'synthetic_settlement_failure'; END IF;RETURN NEW;END $$;CREATE TRIGGER fail_reasoning_event BEFORE INSERT ON agent_supervised.events FOR EACH ROW EXECUTE FUNCTION agent_supervised.fail_reasoning_event();").execute(admin.pool()).await.unwrap();
    let provider = Probe::new("success");
    let c = serde_json::from_value(command(&t, "advance")).unwrap();
    assert!(
        rt.supervised_reasoning_with_provider(
            id(1),
            t["id"].as_str().unwrap().parse().unwrap(),
            &c,
            &provider
        )
        .await
        .is_err()
    );
    sqlx::raw_sql("DROP TRIGGER fail_reasoning_event ON agent_supervised.events;DROP FUNCTION agent_supervised.fail_reasoning_event();").execute(admin.pool()).await.unwrap();
    let reserved = get(s, &t).await;
    assert_eq!(reserved["inference"]["state"], "reserved");
    assert_eq!(reserved["payloads"].as_array().unwrap().len(), 1);
    assert!(
        rt.supervised_reasoning_with_provider(
            id(1),
            t["id"].as_str().unwrap().parse().unwrap(),
            &c,
            &provider
        )
        .await
        .is_ok()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    cmd(a, &reserved, "stop").await;
    // Leave a reserved request across a real restart: it cannot be replayed.
    let t = ready(a, agent).await;
    let mut c = command(&t, "advance");
    c["task_id"] = t["id"].clone();
    c["reservation_id"] = json!(Uuid::new_v4());
    s.supervised(id(1), "reserve_inference", c).await.unwrap();
    // Revocation during another agent's call cannot retain output; usage settlement remains possible.
    let other_agent = other.1["agent_id"].as_str().unwrap();
    let t = http(
        a,
        Some(OTHER),
        "POST",
        &format!("/factory/agents/{other_agent}/tasks"),
        json!({"request_id":Uuid::new_v4(),"input":input()}),
    )
    .await
    .1;
    let mut t = t;
    for op in ["approve", "start", "advance"] {
        let r = http(a, Some(OTHER), "POST", &path(&t), command(&t, op)).await;
        assert_eq!(r.0, 200);
        t = r.1;
    }
    let provider = Probe::new("slow");
    let c = serde_json::from_value(command(&t, "advance")).unwrap();
    let task: Uuid = t["id"].as_str().unwrap().parse().unwrap();
    let (execution, _) = tokio::join!(
        rt.supervised_reasoning_with_provider(id(2), task, &c, &provider),
        async {
            for _ in 0..100 {
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM agent_supervised.inferences WHERE task_id=$1",
                )
                .bind(task)
                .fetch_one(admin.pool())
                .await
                .unwrap();
                if count == 1 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            sqlx::query("DELETE FROM agent_access WHERE agent_id=$1 AND principal_id=$2")
                .bind(other_agent.parse::<Uuid>().unwrap())
                .bind(id(2))
                .execute(admin.pool())
                .await
                .unwrap();
        }
    );
    assert!(execution.is_err());
    let ledger: String =
        sqlx::query_scalar("SELECT state FROM agent_supervised.inferences WHERE task_id=$1")
            .bind(task)
            .fetch_one(admin.pool())
            .await
            .unwrap();
    assert_eq!(ledger, "discarded");
    exercise_real(a, s, admin).await;
    let financial_after: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM simulated_ledger),(SELECT count(*) FROM inference_calls)",
    )
    .fetch_one(admin.pool())
    .await
    .unwrap();
    assert_eq!(financial_before, financial_after);
    assert!(
        sqlx::query("SELECT * FROM agent_supervised.inferences")
            .fetch_all(s.pool())
            .await
            .is_err()
    );
    let invalid_hashes:i64=sqlx::query_scalar("SELECT count(*) FROM agent_supervised.events WHERE sha256<>encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex')").fetch_one(admin.pool()).await.unwrap();
    assert_eq!(invalid_hashes, 0);
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/supervised/reasoning");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("integration-result.json"),serde_json::to_vec_pretty(&json!({"passed":true,"mock_provider_only":true,"paid_requests":0,"two_owner_isolation":true,"approval_and_review":true,"single_request_concurrency":true,"rejection_and_usage":true,"cancellation_and_revocation":true,"atomic_settlement":true,"financial_tables_unchanged":true})).unwrap()).unwrap();
}
pub async fn recovery(a: &Router, s: &Store) {
    let list = http(a, Some(OWNER), "GET", "/factory/agents", Value::Null)
        .await
        .1;
    let agent = list["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["configuration"]["request_id"] == json!(id(2000)))
        .unwrap();
    let tasks = http(
        a,
        Some(OWNER),
        "GET",
        &format!(
            "/factory/agents/{}/tasks",
            agent["agent_id"].as_str().unwrap()
        ),
        Value::Null,
    )
    .await
    .1;
    assert!(
        tasks["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["state"] == "completed"
                && t["input"]["reasoning"].is_object()
                && t["payloads"][2]["memory"].is_object())
    );
    let t = tasks["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["state"] == "running" && t["inference"]["state"] == "reserved")
        .unwrap();
    let provider = Probe::new("success");
    let rt = Runtime::new(s.clone(), Arc::new(Offline));
    let c = serde_json::from_value(t["inference"]["request_body"].clone()).unwrap();
    let restored = rt
        .supervised_reasoning_with_provider(
            id(1),
            t["id"].as_str().unwrap().parse().unwrap(),
            &c,
            &provider,
        )
        .await
        .unwrap();
    assert_eq!(restored, *t);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    let list = http(a, Some(OWNER), "GET", "/factory/agents", Value::Null)
        .await
        .1;
    let agent = list["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["configuration"]["request_id"] == json!(id(4000)))
        .unwrap();
    let tasks = http(
        a,
        Some(OWNER),
        "GET",
        &format!(
            "/factory/agents/{}/tasks",
            agent["agent_id"].as_str().unwrap()
        ),
        Value::Null,
    )
    .await
    .1;
    let t = tasks["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["inference"]["state"] == "reserved")
        .unwrap();
    let mut provider = Probe::new("real_success");
    provider.plan = serde_json::from_value(t["input"]["reasoning"].clone()).unwrap();
    let c = serde_json::from_value(t["inference"]["request_body"].clone()).unwrap();
    let restored = rt
        .supervised_reasoning_with_provider(
            id(1),
            t["id"].as_str().unwrap().parse().unwrap(),
            &c,
            &provider,
        )
        .await
        .unwrap();
    assert_eq!(restored, *t);
    assert_eq!(restored["inference"]["reserved_microusd"], 378);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    println!(
        "mock reasoning result, private memory, reservation and audit persist; no provider replay after restart"
    );
}

fn real_plan() -> ReasoningPlan {
    let mut p = ReasoningPlan::mock();
    p.provider = "openai".into();
    p.model = "gpt-6-luna".into();
    p.max_cost_microusd = 378;
    p.real = Some(RealReasoningScope {
        project: "proj_fixture".into(),
        budget_id: Uuid::new_v4(),
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
    });
    p
}
async fn ready_real(a: &Router, agent: &str, plan: &ReasoningPlan) -> Value {
    let mut input = input();
    input["reasoning"] = json!(plan);
    let r = http(
        a,
        Some(OWNER),
        "POST",
        &format!("/factory/agents/{agent}/tasks"),
        json!({"request_id":Uuid::new_v4(),"input":input}),
    )
    .await;
    assert_eq!(r.0, 200, "real create: {}", r.1);
    let t = cmd(a, &r.1, "approve").await;
    let t = cmd(a, &t, "start").await;
    cmd(a, &t, "advance").await
}
async fn exercise_real(a: &Router, s: &Store, admin: &Store) {
    let mut config = request(4000);
    config["requested_capabilities"] = json!(["read_memory", "remember", "evaluate_goal"]);
    let r = http(a, Some(OWNER), "POST", "/factory/agents", config).await;
    assert_eq!(r.0, 200);
    let agent = r.1["agent_id"].as_str().unwrap();
    let rt = Runtime::new(s.clone(), Arc::new(Offline));
    for mode in [
        "real_success",
        "real_transport",
        "real_slow",
        "real_rejected",
    ] {
        let provider = Probe::new(mode);
        let t = ready_real(a, agent, &provider.plan).await;
        // Default-off HTTP path rejects real transport without reading a credential.
        let blocked = http(a, Some(OWNER), "POST", &path(&t), command(&t, "advance")).await;
        assert_eq!(blocked.0, 403);
        let c: SupervisedCommand = serde_json::from_value(command(&t, "advance")).unwrap();
        let task: Uuid = t["id"].as_str().unwrap().parse().unwrap();
        let (x, y) = tokio::join!(
            rt.supervised_reasoning_with_provider(id(1), task, &c, &provider),
            async {
                if mode == "real_slow" {
                    let reserved = wait_reserved(s, &t).await;
                    wait_dispatched(&provider).await;
                    Some(cmd(a, &reserved, "stop").await)
                } else {
                    None
                }
            }
        );
        let _ = y;
        let mut result = x.unwrap();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(result["inference"]["reserved_microusd"], 378);
        if mode == "real_success" {
            assert_eq!(result["state"], "awaiting_review");
            assert_eq!(result["inference"]["actual_microusd"], 80);
            assert_eq!(result["payloads"][1]["mock_model"], false);
            assert_eq!(result["payloads"][1]["external_requests"], 3);
            assert_eq!(result["payloads"].as_array().unwrap().len(), 2);
            assert_eq!(
                http(
                    a,
                    Some(OTHER),
                    "GET",
                    &format!("/factory/tasks/{task}"),
                    Value::Null
                )
                .await
                .0,
                403
            );
            let repeat = rt
                .supervised_reasoning_with_provider(id(1), task, &c, &provider)
                .await
                .unwrap();
            assert_eq!(repeat, result);
            let mut review = command(&result, "review");
            review["result_sha256"] = result["payloads"][1]["result_sha256"].clone();
            let accepted = http(a, Some(OWNER), "POST", &path(&result), review).await;
            assert_eq!(accepted.0, 200);
            result = accepted.1;
            assert_eq!(result["state"], "completed");
            assert_eq!(result["payloads"][2]["memory"]["kind"], "hypothesis");
        } else {
            if mode == "real_rejected" {
                assert_eq!(result["state"], "failed");
                assert_eq!(result["inference"]["actual_microusd"], 158);
                assert_eq!(
                    result["inference"]["diagnostic"]["response_id"],
                    "resp_fixture123"
                );
                assert_eq!(
                    result["inference"]["diagnostic"]["request_id"],
                    "req_fixture123"
                );
                assert_eq!(
                    result["inference"]["diagnostic"]["response_status"],
                    "incomplete"
                );
                assert_eq!(
                    result["inference"]["diagnostic"]["output_category"],
                    "max_output_tokens"
                );
                assert!(result["inference"]["result_sha256"].is_null());
                let mut review = command(&result, "review");
                review["result_sha256"] = json!("0".repeat(64));
                assert_ne!(
                    http(a, Some(OWNER), "POST", &path(&result), review).await.0,
                    200
                );
                assert_eq!(
                    get(s, &result).await["payloads"].as_array().unwrap().len(),
                    1
                );
            } else {
                assert!(result["inference"]["actual_microusd"].is_null());
            }
            assert_eq!(result["payloads"].as_array().unwrap().len(), 1);
            assert_eq!(
                result["inference"]["state"],
                if matches!(mode, "real_transport" | "real_rejected") {
                    "failed"
                } else {
                    "discarded"
                }
            );
            rt.supervised_reasoning_with_provider(id(1), task, &c, &provider)
                .await
                .unwrap();
            assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        }
        // Reusing an evaluation budget on another task cannot dispatch again.
        let reused = ready_real(a, agent, &provider.plan).await;
        let next = serde_json::from_value(command(&reused, "advance")).unwrap();
        assert!(
            rt.supervised_reasoning_with_provider(
                id(1),
                reused["id"].as_str().unwrap().parse().unwrap(),
                &next,
                &provider
            )
            .await
            .is_err()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        cmd(a, &reused, "stop").await;
    }
    // Persist a real-plan reservation across a database restart; never release it as $0.
    let provider = Probe::new("real_success");
    let t = ready_real(a, agent, &provider.plan).await;
    let mut c = command(&t, "advance");
    c["task_id"] = t["id"].clone();
    c["reservation_id"] = json!(Uuid::new_v4());
    s.supervised(id(1), "reserve_inference", c).await.unwrap();
    let invalid:i64=sqlx::query_scalar("SELECT count(*) FROM agent_supervised.inferences WHERE policy ? 'real' AND reserved_microusd<>378").fetch_one(admin.pool()).await.unwrap();
    assert_eq!(invalid, 0);
}
