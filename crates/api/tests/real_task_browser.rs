//! Test-only browser bridge: authenticated production routes plus explicit mock injection.
//! Run only against the existing disposable Factory PG18 fixture. No HTTP model client.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::IntoResponse,
};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, postgres::PgConnectOptions};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use supa_api::{AppState, OperatorCredential};
use supa_domain::*;
use supa_persistence::{Role, Store, SupervisedCommand, SupervisedCreate};
use supa_providers::{
    Invocation, Offline,
    controlled::{ControlledDispatchGuard, ControlledProvider},
    supervised_mock::SupervisedMock,
};
use supa_runtime::{Runtime, supervised_reasoning::SupervisedStartupAdmission};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;
const OWNER: &str = "factory-synthetic-owner-credential-00000000";
const OTHER: &str = "factory-synthetic-other-credential-00000000";
fn owner() -> Uuid {
    Uuid::from_u128(0x11111111111141118000000000000001)
}
fn admission(t: &Value) -> SupervisedStartupAdmission {
    let p = &t["input"]["reasoning"];
    let settings = BTreeMap::from([
        ("SUPA_SUPERVISED_REAL_MODEL", "true".to_owned()),
        ("SUPA_ALLOW_OPENAI", "true".to_owned()),
        ("OPENAI_API_KEY", "sk-fake-browser".to_owned()),
        ("OPENAI_PROJECT_ID", "proj_offline_browser".to_owned()),
        ("OPENAI_MODEL", "gpt-6-luna".to_owned()),
        ("SUPA_SUPERVISED_REAL_OWNER_ID", owner().to_string()),
        ("SUPA_SUPERVISED_REAL_TASK_ID", t["id"].as_str().unwrap().to_owned()),
        ("SUPA_SUPERVISED_REAL_BUDGET_ID", p["real"]["budget_id"].as_str().unwrap().to_owned()),
        ("SUPA_SUPERVISED_SPENDING_APPROVAL", json!({"owner_id":owner(),"task_id":t["id"],"input_sha256":t["input_sha256"],"budget_id":p["real"]["budget_id"],"project":"proj_offline_browser","model":"gpt-6-luna","expires_at":p["real"]["expires_at"],"per_task_microusd":378,"cumulative_microusd":378}).to_string()),
    ]);
    SupervisedStartupAdmission::from_settings(|k| settings.get(k).cloned())
        .unwrap()
        .unwrap()
}
#[test]
fn synthetic_admission_accepts_short_fake_key() {
    let deadline = chrono::Utc::now() + chrono::Duration::minutes(5);
    let task = json!({"id":Uuid::new_v4(),"input_sha256":"a".repeat(64),"input":{"reasoning":{"real":{"budget_id":Uuid::new_v4(),"expires_at":deadline}}}});
    let checked = admission(&task);
    assert_eq!(checked.owner(), owner());
    assert!(checked.session_covers_scope((deadline.timestamp() + 1) as u64));
}
struct Mock {
    plan: ReasoningPlan,
    calls: Arc<AtomicUsize>,
    uncertain: bool,
}
#[async_trait::async_trait]
impl ControlledProvider for Mock {
    fn policy(&self) -> InferencePolicy {
        self.plan.policy().unwrap()
    }
    fn project(&self) -> &str {
        "proj_offline_browser"
    }
    fn work_evidence_kind(&self) -> &str {
        "openai"
    }
    async fn verify(&self) -> Result<()> {
        Ok(())
    }
    async fn reason(&self, _: &str, _: &[Memory]) -> Invocation<LiveProposal> {
        panic!("background reasoning forbidden")
    }
    async fn supervised_work(
        &self,
        input: &LiveWorkInput,
        guard: &dyn ControlledDispatchGuard,
    ) -> Invocation<WorkProposal> {
        // Simulated metadata, complete-input count, then generation: no network implementation.
        for _ in 0..3 {
            if let Err(e) = guard.authorize().await {
                return Invocation {
                    result: Err(e),
                    usage: None,
                    diagnostic: None,
                };
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
        if self.uncertain {
            return Invocation {
                result: Err(Error::Timeout),
                usage: None,
                diagnostic: None,
            };
        }
        let mut r = SupervisedMock.work(input).await;
        r.usage = Some(TokenUsage {
            input_tokens: 300,
            output_tokens: 100,
            ..Default::default()
        });
        r
    }
}
fn browser_paths(cwd: &Path) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let package = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().ok()?;
    let root = package.join("../..").canonicalize().ok()?;
    let cwd = cwd.canonicalize().ok()?;
    if cwd != package && cwd != root {
        return None;
    }
    if root.join("crates/api").canonicalize().ok()? != package {
        return None;
    }
    let script = root.join("scripts/test-real-task-browser.mjs");
    let metadata = std::fs::symlink_metadata(&script).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    Some((root, script))
}
#[test]
fn browser_launcher_resolves_repository_root() {
    let (root, script) = browser_paths(&std::env::current_dir().unwrap()).unwrap();
    assert_eq!(script, root.join("scripts/test-real-task-browser.mjs"));
    assert!(browser_paths(Path::new("/tmp")).is_none());
    assert!(
        std::process::Command::new("/home/pikoo/.nvm/versions/node/v24.22.1/bin/node")
            .arg("--check")
            .arg(&script)
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
}
#[tokio::test]
async fn denied_guard_has_no_usage_or_mock_operations() {
    struct Denied;
    impl ControlledDispatchGuard for Denied {
        fn authorize(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async { Err(Error::Denied) })
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Mock {
        plan: ReasoningPlan::mock(),
        calls: calls.clone(),
        uncertain: false,
    };
    let input = reasoning_input(
        "Review synthetic incident",
        "recovered",
        "Synthetic service recovered; cause unproven.",
    )
    .unwrap();
    let invocation = provider.supervised_work(&input, &Denied).await;
    assert!(matches!(invocation.result, Err(Error::Denied)));
    assert!(invocation.usage.is_none());
    assert!(invocation.diagnostic.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit socket-only disposable PG18, mock transport, Chromium"]
async fn offline_real_task_browser() {
    for key in [
        "OPENAI_API_KEY",
        "SUPA_SUPERVISED_SPENDING_APPROVAL",
        "SUPA_SUPERVISED_REAL_TASK_ID",
    ] {
        assert!(std::env::var_os(key).is_none());
    }
    let o: PgConnectOptions = std::env::var("SUPA_FACTORY_TEST_URL")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(o.get_database(), Some("supa_factory_test"));
    assert_eq!(o.get_username(), "supa_factory_test");
    assert_eq!(o.get_port(), 55481);
    let socket = o.get_socket().unwrap();
    let parent = socket.parent().unwrap();
    assert_eq!(socket.file_name().unwrap(), "socket");
    assert_eq!(parent.parent(), Some(Path::new("/tmp")));
    let name = parent
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("supa-factory-test-")
        .unwrap();
    assert!(name.len() == 32 && name.bytes().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(socket.canonicalize().unwrap(), socket.to_path_buf());
    let admin = Store::connect(o.to_url_lossy().as_ref()).await.unwrap();
    let (version, listen, address): (String, String, Option<String>) = sqlx::query_as(
        "SELECT version(),current_setting('listen_addresses'),inet_server_addr()::text",
    )
    .fetch_one(admin.pool())
    .await
    .unwrap();
    assert!(version.contains("PostgreSQL 18."));
    assert!(listen.is_empty() && address.is_none());
    // Capture every original real-fixture agent's task JSON, including the restart reservation.
    let preserved:Vec<(Uuid,Value)>=sqlx::query_as("SELECT t.id,agent_supervised.view_task(t) FROM agent_supervised.tasks t JOIN agent_factory.submissions a ON a.agent_id=t.agent_id WHERE a.configuration->>'request_id'='11111111-1111-4111-8000-000000000fa0' ORDER BY t.id").fetch_all(admin.pool()).await.unwrap();
    assert!(!preserved.is_empty());
    let store = Store::connect(o.username("supa_factory_runtime").to_url_lossy().as_ref())
        .await
        .unwrap();
    // This is the already guarded disposable test cluster, never the owner demo.
    // This release rehearsal uses an already-initialized synthetic physical copy.
    // Never install SQL or change grants during this test.
    store.verify_supervised_spending_schema().await.unwrap();
    let runtime = Runtime::new(store.clone(), Arc::new(Offline));
    let state = AppState::new(
        store.clone(),
        runtime.clone(),
        "factory-synthetic-service-credential-0000000".into(),
    )
    .unwrap()
    .with_credentials(vec![
        OperatorCredential {
            id: owner(),
            role: Role::Owner,
            token: OWNER.into(),
        },
        OperatorCredential {
            id: Uuid::from_u128(0x11111111111141118000000000000002),
            role: Role::Owner,
            token: OTHER.into(),
        },
    ])
    .unwrap()
    .with_credential_deadlines(BTreeMap::from([(
        owner(),
        (chrono::Utc::now().timestamp() + 3600) as u64,
    )]))
    .unwrap()
    .with_synthetic_factory()
    .unwrap()
    .with_supervised_tasks()
    .unwrap();
    let app = supa_api::router(state.clone());
    let nonce = Uuid::new_v4().to_string();
    let authorizations = Arc::new(Mutex::new(BTreeMap::<String, Value>::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let bridge = Router::new().fallback({
        let app = app.clone();
        let state = state.clone();
        let runtime = runtime.clone();
        let authorizations = authorizations.clone();
        let calls = calls.clone();
        let nonce = nonce.clone();
        let store = store.clone();
        move |request: Request<Body>| {
            let app = app.clone();
            let state = state.clone();
            let runtime = runtime.clone();
            let authorizations = authorizations.clone();
            let calls = calls.clone();
            let nonce = nonce.clone();
            let store = store.clone();
            async move {
                let path = request.uri().path().to_owned();
                let headers = request.headers().clone();
                let method = request.method().clone();
                let bytes = to_bytes(request.into_body(), 16000).await.unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                let task_id = path.strip_prefix("/factory/tasks/").and_then(|p| {
                    p.strip_suffix("/command")
                        .or_else(|| p.strip_suffix("/spending-consent"))
                });
                let mut registered = false;
                if let Some(tid) = task_id {
                    let mut get = Request::builder().uri(format!("/factory/tasks/{tid}"));
                    *get.headers_mut().unwrap() = headers.clone();
                    let response = app
                        .clone()
                        .oneshot(get.body(Body::empty()).unwrap())
                        .await
                        .unwrap();
                    if response.status() == StatusCode::OK {
                        let current: Value = serde_json::from_slice(
                            &to_bytes(response.into_body(), 1_000_000).await.unwrap(),
                        )
                        .unwrap();
                        registered = current["creator_id"] == json!(owner())
                            && current["input"]["task"].as_str().is_some_and(|s| {
                                s.starts_with(&format!("Phase8 offline {nonce} "))
                            });
                        if registered && path.ends_with("/spending-consent") {
                            let auth = authorizations.lock().await.get(tid).cloned();
                            if let Some(auth) = auth {
                                let scoped = supa_api::router(
                                    state
                                        .with_supervised_spending_scope(admission(&auth))
                                        .unwrap(),
                                );
                                let mut forward = Request::builder().method(method).uri(&path);
                                *forward.headers_mut().unwrap() = headers;
                                return scoped
                                    .oneshot(forward.body(Body::from(bytes)).unwrap())
                                    .await
                                    .unwrap();
                            }
                        }
                        if registered && body["operation"] == "advance" && current["step"] == 1 {
                            let auth = authorizations.lock().await.get(tid).cloned();
                            if !auth
                                .as_ref()
                                .is_some_and(|a| a["input_sha256"] == current["input_sha256"])
                            {
                                return StatusCode::FORBIDDEN.into_response();
                            }
                            let _checked = admission(&current); // Independently bound, expiring synthetic receipt; never installed in production.
                            let mut denied = Request::builder().method(method.clone()).uri(&path);
                            *denied.headers_mut().unwrap() = headers.clone();
                            assert_eq!(
                                app.clone()
                                    .oneshot(denied.body(Body::from(bytes.clone())).unwrap())
                                    .await
                                    .unwrap()
                                    .status(),
                                StatusCode::FORBIDDEN,
                                "production generation must remain disabled"
                            );
                            let mut unchanged =
                                Request::builder().uri(format!("/factory/tasks/{tid}"));
                            *unchanged.headers_mut().unwrap() = headers.clone();
                            let response = app
                                .clone()
                                .oneshot(unchanged.body(Body::empty()).unwrap())
                                .await
                                .unwrap();
                            assert_eq!(response.status(), StatusCode::OK);
                            let after_denial: Value = serde_json::from_slice(
                                &to_bytes(response.into_body(), 1_000_000).await.unwrap(),
                            )
                            .unwrap();
                            assert_eq!(
                                after_denial, current,
                                "missing receipt must not reserve spending or mutate the task"
                            );
                            let command: SupervisedCommand =
                                serde_json::from_value(body.clone()).unwrap();
                            let plan: ReasoningPlan =
                                serde_json::from_value(current["input"]["reasoning"].clone())
                                    .unwrap();
                            let result = runtime
                                .supervised_consented_reasoning_with_provider(
                                    owner(),
                                    tid.parse().unwrap(),
                                    &command,
                                    &Mock {
                                        plan,
                                        calls,
                                        uncertain: false,
                                    },
                                )
                                .await
                                .unwrap();
                            return axum::Json(result).into_response();
                        }
                    }
                }
                let mut forward = Request::builder().method(method).uri(&path);
                *forward.headers_mut().unwrap() = headers;
                let response = app
                    .oneshot(forward.body(Body::from(bytes)).unwrap())
                    .await
                    .unwrap();
                if registered
                    && body["operation"] == "approve"
                    && response.status() == StatusCode::OK
                {
                    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
                    let task: Value = serde_json::from_slice(&bytes).unwrap();
                    admission(&task).verify_task(&store).await.unwrap();
                    authorizations
                        .lock()
                        .await
                        .insert(task["id"].as_str().unwrap().to_owned(), task);
                    return (
                        StatusCode::OK,
                        [("content-type", "application/json")],
                        bytes,
                    )
                        .into_response();
                }
                response
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, bridge).await.unwrap();
    });
    let (root, script) = browser_paths(&std::env::current_dir().unwrap())
        .expect("verified_repository_root_and_script_required");
    let browser_nonce = nonce.clone();
    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new("/home/pikoo/.nvm/versions/node/v24.22.1/bin/node")
            .arg(script)
            .current_dir(root)
            .env("SUPA_BACKEND_URL", format!("http://{address}"))
            .env("SUPA_OFFLINE_BROWSER_NONCE", browser_nonce)
            .status()
            .unwrap()
    })
    .await
    .unwrap();
    server.abort();
    assert!(status.success(), "offline browser gate failed; no retry");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        6,
        "two tasks: two simulated preflights and one generation each, no replay"
    );
    for (id, original) in preserved {
        let current: Value = sqlx::query_scalar(
            "SELECT agent_supervised.view_task(t) FROM agent_supervised.tasks t WHERE id=$1",
        )
        .bind(id)
        .fetch_one(admin.pool())
        .await
        .unwrap();
        assert_eq!(current, original, "original restart fixture changed");
    }
    let valid:bool=sqlx::query_scalar("SELECT bool_and(sha256=encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex')) FROM agent_supervised.events").fetch_one(admin.pool()).await.unwrap();
    assert!(valid);
    let linked:bool=sqlx::query_scalar("SELECT bool_and(previous_sha256=coalesce(previous,repeat('0',64))) FROM (SELECT previous_sha256,lag(sha256) OVER(PARTITION BY task_id ORDER BY revision) previous FROM agent_supervised.events) q").fetch_one(admin.pool()).await.unwrap();
    assert!(linked);
    spending_security_checks(&store, &admin, &runtime, &nonce).await;
    println!(
        "desktop/mobile mock execution; 6 simulated operations; 0 network provider requests; original restart tasks unchanged"
    );
}

async fn security_task(s: &Store, agent: Uuid, template: &Value, ttl: i64) -> Value {
    let mut input = template.clone();
    input["task"] = json!(format!("Review synthetic consent {}", Uuid::new_v4()));
    input["retain_memory"] = json!(false);
    input["reasoning"]["real"]["budget_id"] = json!(Uuid::new_v4());
    input["reasoning"]["real"]["expires_at"] =
        json!(chrono::Utc::now() + chrono::Duration::seconds(ttl));
    let created = s
        .supervised_create(
            owner(),
            agent,
            &serde_json::from_value::<SupervisedCreate>(
                json!({"request_id":Uuid::new_v4(),"input":input}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    s.supervised(owner(), "approve", security_body(&created))
        .await
        .unwrap()
}
fn security_body(t: &Value) -> Value {
    json!({"task_id":t["id"],"request_id":Uuid::new_v4(),"expected_revision":t["revision"],"input_sha256":t["input_sha256"],"confirmed":true})
}
async fn security_start(s: &Store, runtime: &Runtime, t: &Value) -> Value {
    let t = s
        .supervised(owner(), "get", json!({"task_id":t["id"]}))
        .await
        .unwrap();
    let t = s
        .supervised(owner(), "start", security_body(&t))
        .await
        .unwrap();
    // Build only the public command contract; no caller-supplied provider result.
    let mut body = security_body(&t);
    body.as_object_mut().unwrap().remove("task_id");
    body["operation"] = json!("advance");
    runtime
        .supervised_command(
            owner(),
            t["id"].as_str().unwrap().parse().unwrap(),
            &serde_json::from_value(body).unwrap(),
        )
        .await
        .unwrap()
}
async fn security_issue(s: &Store, t: &Value) -> Value {
    let scope = admission(t);
    let mut body = security_body(t);
    body["receipt"] = scope
        .consent_receipt(
            s,
            owner(),
            t["id"].as_str().unwrap().parse().unwrap(),
            t["input_sha256"].as_str().unwrap(),
        )
        .await
        .unwrap();
    s.supervised_spending(owner(), "issue", body).await.unwrap()
}
async fn spending_security_checks(s: &Store, admin: &Store, runtime: &Runtime, nonce: &str) {
    let (agent,template):(Uuid,Value)=sqlx::query_as("SELECT agent_id,input FROM agent_supervised.tasks WHERE input->>'task' LIKE $1 ORDER BY created_at DESC LIMIT 1")
        .bind(format!("Phase8 offline {nonce} %")).fetch_one(admin.pool()).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let task = security_task(s, agent, &template, 300).await;
    let running = security_start(s, runtime, &task).await;
    let mut cmd = security_body(&running);
    cmd.as_object_mut().unwrap().remove("task_id");
    cmd["operation"] = json!("advance");
    let cmd: SupervisedCommand = serde_json::from_value(cmd).unwrap();
    let provider = Mock {
        plan: serde_json::from_value(running["input"]["reasoning"].clone()).unwrap(),
        calls: calls.clone(),
        uncertain: false,
    };
    assert!(
        runtime
            .supervised_consented_reasoning_with_provider(
                owner(),
                serde_json::from_value(task["id"].clone()).unwrap(),
                &cmd,
                &provider
            )
            .await
            .is_err()
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "ordinary task approval never spends"
    );
    s.supervised(owner(), "stop", security_body(&running))
        .await
        .unwrap();
    let task = security_task(s, agent, &template, 300).await;
    let scope = admission(&task);
    let mut valid = security_body(&task);
    valid["receipt"] = scope
        .consent_receipt(
            s,
            owner(),
            serde_json::from_value(task["id"].clone()).unwrap(),
            task["input_sha256"].as_str().unwrap(),
        )
        .await
        .unwrap();
    let other = Uuid::from_u128(0x11111111111141118000000000000002);
    assert!(
        s.supervised_spending(other, "issue", valid.clone())
            .await
            .is_err()
    );
    for (key, value) in [
        ("input_sha256", json!("0".repeat(64))),
        ("project", json!("proj_wrong")),
        ("model", json!("wrong")),
        ("per_task_microusd", json!(379)),
    ] {
        let mut bad = valid.clone();
        bad["request_id"] = json!(Uuid::new_v4());
        if key == "input_sha256" {
            bad[key] = value;
        } else {
            bad["receipt"][key] = value;
        }
        assert!(
            s.supervised_spending(owner(), "issue", bad).await.is_err(),
            "deny {key}"
        );
    }
    let mut second = valid.clone();
    second["request_id"] = json!(Uuid::new_v4());
    let (a, b) = tokio::join!(
        s.supervised_spending(owner(), "issue", valid.clone()),
        s.supervised_spending(owner(), "issue", second)
    );
    assert_ne!(a.is_ok(), b.is_ok(), "exactly one concurrent consent wins");
    assert!(
        s.supervised_spending(owner(), "issue", valid)
            .await
            .is_err()
    );
    let running = security_start(s, runtime, &task).await;
    let mut cmd = security_body(&running);
    cmd.as_object_mut().unwrap().remove("task_id");
    cmd["operation"] = json!("advance");
    let cmd: SupervisedCommand = serde_json::from_value(cmd).unwrap();
    let provider = Mock {
        plan: serde_json::from_value(running["input"]["reasoning"].clone()).unwrap(),
        calls: calls.clone(),
        uncertain: false,
    };
    let done = runtime
        .supervised_consented_reasoning_with_provider(
            owner(),
            serde_json::from_value(task["id"].clone()).unwrap(),
            &cmd,
            &provider,
        )
        .await
        .unwrap();
    assert_eq!(done["state"], "awaiting_review");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(
        runtime
            .supervised_consented_reasoning_with_provider(
                owner(),
                serde_json::from_value(task["id"].clone()).unwrap(),
                &cmd,
                &provider
            )
            .await
            .is_err()
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "consumed consent cannot replay"
    );
    s.supervised(owner(), "stop", security_body(&done))
        .await
        .unwrap();
    let task = security_task(s, agent, &template, 300).await;
    security_issue(s, &task).await;
    let t = s
        .supervised(owner(), "get", json!({"task_id":task["id"]}))
        .await
        .unwrap();
    s.supervised_spending(owner(), "revoke", security_body(&t))
        .await
        .unwrap();
    assert!(
        s.supervised_spending(owner(), "receipt", json!({"task_id":task["id"]}))
            .await
            .is_err()
    );
    let task = security_task(s, agent, &template, 300).await;
    security_issue(s, &task).await;
    let running = security_start(s, runtime, &task).await;
    let mut cmd = security_body(&running);
    cmd.as_object_mut().unwrap().remove("task_id");
    cmd["operation"] = json!("advance");
    let cmd: SupervisedCommand = serde_json::from_value(cmd).unwrap();
    let provider = Mock {
        plan: serde_json::from_value(running["input"]["reasoning"].clone()).unwrap(),
        calls: calls.clone(),
        uncertain: true,
    };
    let uncertain = runtime
        .supervised_consented_reasoning_with_provider(
            owner(),
            serde_json::from_value(task["id"].clone()).unwrap(),
            &cmd,
            &provider,
        )
        .await
        .unwrap();
    assert_eq!(uncertain["state"], "failed");
    assert!(uncertain["inference"]["actual_microusd"].is_null());
    assert_eq!(uncertain["inference"]["reserved_microusd"], 378);
    let before = calls.load(Ordering::SeqCst);
    assert!(
        runtime
            .supervised_consented_reasoning_with_provider(
                owner(),
                serde_json::from_value(task["id"].clone()).unwrap(),
                &cmd,
                &provider
            )
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), before);
    let task = security_task(s, agent, &template, 5).await;
    security_issue(s, &task).await;
    tokio::time::sleep(std::time::Duration::from_millis(5100)).await;
    let status = s
        .supervised_spending(owner(), "status", json!({"task_id":task["id"]}))
        .await
        .unwrap();
    assert_eq!(status["consent"]["state"], "expired");
    assert!(
        s.supervised_spending(owner(), "receipt", json!({"task_id":task["id"]}))
            .await
            .is_err()
    );
    let t = s
        .supervised(owner(), "get", json!({"task_id":task["id"]}))
        .await
        .unwrap();
    s.supervised(owner(), "stop", security_body(&t))
        .await
        .unwrap();
    verify_spending_history(admin).await;
    for table in ["spending_consents", "spending_decisions"] {
        assert!(
            sqlx::query(&format!("DELETE FROM agent_supervised.{table}"))
                .execute(s.pool())
                .await
                .is_err()
        );
        let mut tx = admin.pool().begin().await.unwrap();
        assert!(
            sqlx::query(&format!("DELETE FROM agent_supervised.{table}"))
                .execute(&mut *tx)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
    }
    println!(
        "durable consent: missing/foreign/hash/project/model/ceiling/duplicate/concurrent/revoked/expired/replay/unknown-cost/append-only gates passed"
    );
}
async fn verify_spending_history(admin: &Store) {
    let valid:bool=sqlx::query_scalar("SELECT bool_and(sha256=encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex')) FROM agent_supervised.spending_decisions").fetch_one(admin.pool()).await.unwrap();
    assert!(valid);
    let linked:bool=sqlx::query_scalar("SELECT bool_and(previous_sha256=coalesce(previous,repeat('0',64))) FROM (SELECT previous_sha256,lag(sha256) OVER(PARTITION BY actor ORDER BY sequence) previous FROM agent_supervised.spending_decisions) q").fetch_one(admin.pool()).await.unwrap();
    assert!(linked);
}
#[tokio::test]
#[ignore = "explicit disposable PostgreSQL restart verification"]
async fn spending_restart_recovery() {
    let o: PgConnectOptions = std::env::var("SUPA_FACTORY_TEST_URL")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(o.get_database(), Some("supa_factory_test"));
    assert_eq!(o.get_port(), 55481);
    assert!(
        o.get_socket()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("/tmp/supa-factory-test-")
    );
    let admin = Store::connect(o.to_url_lossy().as_ref()).await.unwrap();
    let (version, listen): (String, String) =
        sqlx::query_as("SELECT version(),current_setting('listen_addresses')")
            .fetch_one(admin.pool())
            .await
            .unwrap();
    assert!(version.contains("PostgreSQL 18."));
    assert!(listen.is_empty());
    verify_spending_history(&admin).await;
    let (consumed,revoked,unknown):(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM agent_supervised.spending_consents WHERE reservation_id IS NOT NULL),(SELECT count(*) FROM agent_supervised.spending_consents WHERE revoked_at IS NOT NULL),(SELECT count(*) FROM agent_supervised.inferences i JOIN agent_supervised.spending_consents c USING(task_id) WHERE i.actual_microusd IS NULL AND c.reservation_id IS NOT NULL)").fetch_one(admin.pool()).await.unwrap();
    assert!(consumed >= 4 && revoked >= 1 && unknown >= 1);
    let s = Store::connect(o.username("supa_factory_runtime").to_url_lossy().as_ref())
        .await
        .unwrap();
    s.verify_supervised_spending_schema().await.unwrap();
    let tasks:Vec<Uuid>=sqlx::query_scalar("SELECT task_id FROM agent_supervised.spending_consents WHERE reservation_id IS NOT NULL OR revoked_at IS NOT NULL").fetch_all(admin.pool()).await.unwrap();
    for task in tasks {
        assert!(
            s.supervised_spending(owner(), "receipt", json!({"task_id":task}))
                .await
                .is_err()
        );
    }
    println!(
        "consumed/revoked/unknown reservations and decision chain survive actual PostgreSQL restart without replay"
    );
}
