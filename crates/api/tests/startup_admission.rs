//! Actual API executable, disposable PG18, synthetic keys, no generation requests.
use serde_json::{Value, json};
use sqlx::{ConnectOptions, postgres::PgConnectOptions};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use supa_persistence::{FactoryRequest, Store, SupervisedCreate};
use uuid::Uuid;
const OWNER: &str = "fixture-startup-owner-credential-00000000";
const OTHER: &str = "fixture-startup-other-credential-00000000";
const SERVICE: &str = "fixture-startup-service-credential-0000000";
const SCOPE: [&str; 4] = [
    "SUPA_SUPERVISED_REAL_OWNER_ID",
    "SUPA_SUPERVISED_REAL_TASK_ID",
    "SUPA_SUPERVISED_REAL_BUDGET_ID",
    "SUPA_SUPERVISED_SPENDING_APPROVAL",
];
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        // Only this test's child. No system-wide process matching or cleanup.
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        for _ in 0..100 {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn child(env: &BTreeMap<String, String>) -> Process {
    Process(
        Command::new(env!("CARGO_BIN_EXE_supa-api"))
            .env_clear()
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn request(port: u16, path: &str, token: Option<&str>, body: Option<&Value>) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let data = body.map(Value::to_string).unwrap_or_default();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    write!(stream,"{} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{data}",if body.is_some(){"POST"}else{"GET"},data.len()).unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        serde_json::from_str(body).unwrap_or(Value::Null),
    )
}
fn ready(env: &BTreeMap<String, String>) -> Process {
    let mut process = child(env);
    let port = env["SUPA_BIND"]
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(8);
    while Instant::now() < until {
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "API exited before readiness"
        );
        if TcpStream::connect(("127.0.0.1", port)).is_ok()
            && request(port, "/ready", None, None).0 == 200
        {
            return process;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("API readiness deadline");
}
fn denied(env: &BTreeMap<String, String>) {
    let mut process = child(env);
    let until = Instant::now() + Duration::from_secs(8);
    while Instant::now() < until {
        if let Some(status) = process.0.try_wait().unwrap() {
            assert!(!status.success());
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("inconsistent startup configuration was not rejected");
}
async fn snapshot(admin: &Store) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('tasks',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM agent_supervised.tasks t),'inferences',(SELECT jsonb_agg(to_jsonb(t) ORDER BY task_id) FROM agent_supervised.inferences t),'commands',(SELECT count(*) FROM agent_supervised.commands),'events',(SELECT count(*) FROM agent_supervised.events),'financial',(SELECT count(*) FROM simulated_ledger),'calls',(SELECT count(*) FROM inference_calls))")
        .fetch_one(admin.pool()).await.unwrap()
}
async fn op(store: &Store, owner: Uuid, task: &Value, operation: &str) -> Value {
    store.supervised(owner,operation,json!({"task_id":task["id"],"request_id":Uuid::new_v4(),"expected_revision":task["revision"],"input_sha256":task["input_sha256"],"confirmed":true})).await.unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit disposable PG18 runner after supervised integration; no live services"]
async fn actual_process_admission_and_restricted_recovery() {
    let url = std::env::var("SUPA_FACTORY_TEST_URL").expect("disposable PG18 URL required");
    let parsed: PgConnectOptions = url.parse().unwrap();
    assert_eq!(parsed.get_database(), Some("supa_factory_test"));
    assert_eq!(parsed.get_username(), "supa_factory_test");
    assert_eq!(parsed.get_port(), 55481);
    let socket = parsed.get_socket().unwrap();
    let work = socket.parent().unwrap();
    assert_eq!(socket.file_name().unwrap(), "socket");
    assert_eq!(work.parent(), Some(Path::new("/tmp")));
    let name = work
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("supa-factory-test-")
        .unwrap();
    assert!(name.len() == 32 && name.bytes().all(|x| x.is_ascii_hexdigit()));
    assert_eq!(socket.canonicalize().unwrap(), socket.to_path_buf());
    let admin = Store::connect(&url).await.unwrap();
    let runtime_url = parsed
        .username("supa_factory_runtime")
        .to_url_lossy()
        .to_string();
    let store = Store::connect(&runtime_url).await.unwrap();
    store.verify_runtime_schema().await.unwrap();
    store.verify_supervised_schema().await.unwrap();
    let owner = Uuid::new_v4();
    let other = Uuid::new_v4();
    let config:FactoryRequest=serde_json::from_value(json!({"request_id":Uuid::new_v4(),"name":"Synthetic startup admission","description":"Disposable process test only","purpose":"Review synthetic incident notes","personality":{"voice":"Concise","traits":["careful"]},"goals":["Review synthetic notes"],"requested_capabilities":[],"autonomy":"disabled","spending_limit_microusd":0,"private_consent":true})).unwrap();
    let agent = store.factory_create(owner, &config).await.unwrap();
    let expires = chrono::Utc::now() + chrono::Duration::minutes(5);
    let budget = Uuid::new_v4();
    let plan = json!({"provider":"openai","model":"gpt-6-luna","max_input_tokens":2000,"max_output_tokens":256,"max_requests":1,"max_cost_microusd":378,"external_tools":[],"real":{"project":"proj_process_fixture","budget_id":budget,"expires_at":expires}});
    let create:SupervisedCreate=serde_json::from_value(json!({"request_id":Uuid::new_v4(),"input":{"task":"Inspect synthetic startup fixture","desired_outcome":"Review synthetic observations","material":"Synthetic service recovered; cause unverified.","synthetic_only":true,"retain_memory":false,"reasoning":plan}})).unwrap();
    let task = store
        .supervised_create(
            owner,
            serde_json::from_value(agent["agent_id"].clone()).unwrap(),
            &create,
        )
        .await
        .unwrap();
    let task = op(&store, owner, &task, "approve").await;
    let receipt = json!({"owner_id":owner,"task_id":task["id"],"input_sha256":task["input_sha256"],"budget_id":budget,"project":"proj_process_fixture","model":"gpt-6-luna","expires_at":expires,"per_task_microusd":378,"cumulative_microusd":378});
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    proxy.set_nonblocking(true).unwrap();
    let bind = port();
    let mut settings=BTreeMap::from([
        ("PATH".into(),"/usr/bin:/bin".into()),("DATABASE_URL".into(),runtime_url),
        ("SUPA_PRIVATE_STAGING".into(),"true".into()),("SUPA_SYNTHETIC_FACTORY_ENABLED".into(),"true".into()),
        ("SUPA_SUPERVISED_TASKS_ENABLED".into(),"true".into()),("SUPA_PROVIDER".into(),"offline".into()),
        ("SUPA_BIND".into(),format!("127.0.0.1:{bind}")),("SUPA_API_TOKEN".into(),SERVICE.into()),
        ("SUPA_OPERATOR_CREDENTIALS".into(),json!([{"id":owner,"role":"owner","token":OWNER},{"id":other,"role":"owner","token":OTHER}]).to_string()),
        ("SUPA_OPERATOR_SESSION_DEADLINES".into(),json!({owner.to_string():expires.timestamp()+60,other.to_string():expires.timestamp()+60}).to_string()),
        ("SUPA_ALLOW_OPENAI".into(),"true".into()),("SUPA_SUPERVISED_REAL_MODEL".into(),"true".into()),
        ("SUPA_SUPERVISED_REAL_OWNER_ID".into(),owner.to_string()),("SUPA_SUPERVISED_REAL_TASK_ID".into(),task["id"].as_str().unwrap().into()),
        ("SUPA_SUPERVISED_REAL_BUDGET_ID".into(),budget.to_string()),("SUPA_SUPERVISED_SPENDING_APPROVAL".into(),receipt.to_string()),
        ("OPENAI_MODEL".into(),"gpt-6-luna".into()),("OPENAI_PROJECT_ID".into(),"proj_process_fixture".into()),
        ("OPENAI_API_KEY".into(),"fixture-synthetic-provider-credential".into()),
        ("HTTP_PROXY".into(),format!("http://{}",proxy.local_addr().unwrap())),("HTTPS_PROXY".into(),format!("http://{}",proxy.local_addr().unwrap())),
    ]);
    let before = snapshot(&admin).await;
    let mut cases = 0;
    for key in [
        "SUPA_SUPERVISED_REAL_MODEL",
        "SUPA_ALLOW_OPENAI",
        "SUPA_SUPERVISED_REAL_TASK_ID",
        "SUPA_SUPERVISED_REAL_OWNER_ID",
        "SUPA_SUPERVISED_REAL_BUDGET_ID",
        "SUPA_SUPERVISED_SPENDING_APPROVAL",
        "OPENAI_MODEL",
        "OPENAI_PROJECT_ID",
        "OPENAI_API_KEY",
    ] {
        let mut bad = settings.clone();
        bad.remove(key);
        denied(&bad);
        cases += 1;
    }
    for key in [
        "SUPA_ALLOW_ANTHROPIC",
        "SUPA_CONTROLLED_INFERENCE",
        "SUPA_CONTROLLED_WORK",
        "SUPA_WORKER_ENABLED",
        "SUPA_API_SCHEDULER_ENABLED",
        "SUPA_WALLET_ENABLED",
        "SUPA_SOCIAL_REVIEW_ENABLED",
    ] {
        let mut bad = settings.clone();
        bad.insert(key.into(), "true".into());
        denied(&bad);
        cases += 1;
    }
    for (key, value) in [
        ("SUPA_PROVIDER", "openai"),
        ("SUPA_PRIVATE_STAGING", "false"),
        ("SUPA_SYNTHETIC_FACTORY_ENABLED", "false"),
        ("SUPA_SUPERVISED_TASKS_ENABLED", "false"),
        ("SUPA_ALLOW_OPENAI", "false"),
        ("SUPA_SUPERVISED_REAL_MODEL", "false"),
        ("SUPA_ALLOW_OPENAI", "yes"),
    ] {
        let mut bad = settings.clone();
        bad.insert(key.into(), value.into());
        denied(&bad);
        cases += 1;
    }
    for (field, value) in [
        ("input_sha256", json!("a".repeat(64))),
        ("owner_id", json!(other)),
        ("task_id", json!(Uuid::new_v4())),
        ("budget_id", json!(Uuid::new_v4())),
        ("model", json!("other")),
        ("project", json!("proj_wrong")),
        ("per_task_microusd", json!(377)),
        ("cumulative_microusd", json!(379)),
        (
            "expires_at",
            json!(chrono::Utc::now() - chrono::Duration::seconds(1)),
        ),
    ] {
        let mut bad = settings.clone();
        let mut invalid = receipt.clone();
        invalid[field] = value;
        bad.insert(
            "SUPA_SUPERVISED_SPENDING_APPROVAL".into(),
            invalid.to_string(),
        );
        denied(&bad);
        cases += 1;
    }
    let mut bad = settings.clone();
    // Internally consistent server settings must still match the persisted task.
    for (setting, field, value) in [
        (
            "SUPA_SUPERVISED_REAL_OWNER_ID",
            "owner_id",
            other.to_string(),
        ),
        (
            "SUPA_SUPERVISED_REAL_TASK_ID",
            "task_id",
            Uuid::new_v4().to_string(),
        ),
        (
            "SUPA_SUPERVISED_REAL_BUDGET_ID",
            "budget_id",
            Uuid::new_v4().to_string(),
        ),
        ("OPENAI_PROJECT_ID", "project", "proj_other_fixture".into()),
    ] {
        let mut bad = settings.clone();
        let mut invalid = receipt.clone();
        invalid[field] = json!(value);
        bad.insert(setting.into(), value);
        bad.insert(
            "SUPA_SUPERVISED_SPENDING_APPROVAL".into(),
            invalid.to_string(),
        );
        denied(&bad);
        cases += 1;
    }
    bad.insert(
        "SUPA_OPERATOR_CREDENTIALS".into(),
        json!([{"id":owner,"role":"viewer","token":OWNER}]).to_string(),
    );
    denied(&bad);
    cases += 1;
    let mut bad = settings.clone();
    bad.insert(
        "SUPA_OPERATOR_SESSION_DEADLINES".into(),
        json!({owner.to_string():1}).to_string(),
    );
    denied(&bad);
    cases += 1;
    assert_eq!(
        snapshot(&admin).await,
        before,
        "rejected startup must not write data"
    );
    {
        let _api = ready(&settings);
        let path = format!("/factory/tasks/{}", task["id"].as_str().unwrap());
        assert_eq!(request(bind, &path, Some(OWNER), None), (200, task.clone()));
        assert_eq!(request(bind, &path, None, None).0, 401);
        assert!(matches!(
            request(bind, &path, Some(OTHER), None).0,
            403 | 404
        ));
        let command = json!({"request_id":Uuid::new_v4(),"operation":"advance","expected_revision":task["revision"],"input_sha256":task["input_sha256"],"confirmed":false});
        assert_eq!(
            request(bind, &(path + "/command"), Some(OWNER), Some(&command)).0,
            403
        );
    }
    assert_eq!(
        snapshot(&admin).await,
        before,
        "successful startup and unapproved dispatch must not reserve or execute"
    );
    // A durable synthetic reservation must never be replayed during startup.
    let running = op(&store, owner, &task, "start").await;
    let inspected=store.supervised(owner,"advance",json!({"task_id":task["id"],"request_id":Uuid::new_v4(),"expected_revision":running["revision"],"input_sha256":task["input_sha256"],"payload":{"action":"inspect_input","external_requests":0,"spending_microusd":0}})).await.unwrap();
    let reserved=store.supervised(owner,"reserve_inference",json!({"task_id":task["id"],"request_id":Uuid::new_v4(),"reservation_id":Uuid::new_v4(),"expected_revision":inspected["revision"],"input_sha256":task["input_sha256"],"operation":"advance","confirmed":false})).await.unwrap();
    assert_eq!(reserved["inference"]["reserved_microusd"], 378);
    assert!(reserved["inference"]["actual_microusd"].is_null());
    denied(&settings);
    cases += 1;
    let recovery_before = snapshot(&admin).await;
    settings.insert("SUPA_ALLOW_OPENAI".into(), "false".into());
    settings.insert("SUPA_SUPERVISED_REAL_MODEL".into(), "false".into());
    // A stale receipt must be removed, not treated as disabled authorization.
    denied(&settings);
    cases += 1;
    for key in SCOPE {
        settings.insert(key.into(), String::new());
    }
    settings.insert("OPENAI_API_KEY".into(), "unused invalid fixture key".into());
    for _ in 0..2 {
        let _api = ready(&settings);
        assert_eq!(
            request(
                bind,
                &format!("/factory/tasks/{}", task["id"].as_str().unwrap()),
                Some(OWNER),
                None
            ),
            (200, reserved.clone())
        );
    }
    assert_eq!(
        snapshot(&admin).await,
        recovery_before,
        "restricted recovery must preserve uncertain reservations"
    );
    let stopped = op(&store, owner, &reserved, "stop").await;
    assert_eq!(stopped["state"], "stopped");
    assert_eq!(stopped["inference"], reserved["inference"]);
    assert!(
        proxy.accept().is_err(),
        "startup/recovery must make no provider requests"
    );
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/startup-admission");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("process-result.json"),serde_json::to_vec_pretty(&json!({"passed":true,"denied_cases":cases,"actual_api_process":true,"restricted_postgres_role":true,"exact_task_startup":true,"anonymous_and_cross_owner_denied":true,"unapproved_execution_denied":true,"startup_makes_no_writes":true,"reserved_request_not_replayed":true,"uncertain_reservation_preserved_across_two_restarts":true,"disabled_credentials_not_read":true,"provider_connections":0,"paid_requests":0})).unwrap()).unwrap();
}
