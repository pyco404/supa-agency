use super::*;
use std::sync::{Arc, Mutex};
fn receipt() -> Receipt {
    let expiry = now().unwrap() + 600;
    let account = MockAccount {
        id: "123".into(),
        username: "supa_agency".into(),
        mode: "live".into(),
    };
    Receipt {
        version: 1,
        receipt_id: Uuid::from_u128(8),
        owner_id: OWNER.parse().unwrap(),
        local_uid: std::fs::metadata("/proc/self").unwrap().uid(),
        client_id: "fixture_client".into(),
        account: account.clone(),
        callback: crate::x_oauth::REGISTERED_CALLBACK.into(),
        custody: crate::x_oauth::CREDENTIAL_CUSTODY.into(),
        scope: "original_text_released_metadata_only".into(),
        owner_proof_sha256: "a".repeat(64),
        owner_verification_approved: true,
        autonomous_activation_approved: true,
        issued_unix: now().unwrap(),
        expires_unix: expiry,
        grant: json!({"grant_id":Uuid::from_u128(9),"account":account,"confirmed":true,"scope":"original_text_released_metadata_only",
            "expires_at":chrono::DateTime::from_timestamp(expiry as i64,0).unwrap().to_rfc3339(),"max_posts":6,"interval_seconds":14400,"budget_microusd":150000,"activation_receipt_id":Uuid::from_u128(8),"owner_proof_sha256":"a".repeat(64)}),
        identity_read_microusd: 10000,
        post_create_microusd: 15000,
    }
}
#[test]
fn activation_requires_exact_verified_owner_account_scope_expiry_caps_and_prices() {
    let r = receipt();
    r.validate(r.receipt_id, now().unwrap()).unwrap();
    for (key, bad) in [
        ("owner_id", json!(Uuid::from_u128(1))),
        ("owner_verification_approved", json!(false)),
        ("autonomous_activation_approved", json!(false)),
        ("expires_unix", json!(1)),
        ("callback", json!("https://example.invalid")),
        ("post_create_microusd", json!(1)),
        ("owner_proof_sha256", json!("bad")),
        ("scope", json!("connection_only")),
    ] {
        let mut v = serde_json::to_value(receipt()).unwrap();
        v[key] = bad;
        let r: Receipt = serde_json::from_value(v).unwrap();
        assert!(r.validate(r.receipt_id, now().unwrap()).is_err());
    }
    for (key, bad) in [
        ("max_posts", json!(7)),
        ("interval_seconds", json!(14399)),
        ("budget_microusd", json!(24999)),
        ("expires_at", json!("bad")),
        ("confirmed", json!(false)),
        ("grant_id", json!(Uuid::nil())),
    ] {
        let mut r = receipt();
        r.grant[key] = bad;
        assert!(r.validate(r.receipt_id, now().unwrap()).is_err());
    }
    for bad in ["0", "01", "-1", "18446744073709551616", "123x", ""] {
        assert!(!numeric_id(bad));
    }
    assert!(LiveAuthorization::load(Uuid::new_v4()).is_err());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("receipt"), "{}").unwrap();
    assert!(protected(&dir.path().join("receipt"), false).is_err());
}
#[test]
fn connection_never_accepts_other_owner_client_account_scope_or_expired_token() {
    let auth = LiveAuthorization {
        receipt: receipt(),
        digest: "not-installed".into(),
    };
    let payload = json!({"owner":auth.owner(),"client_id":"fixture_client","account_id":"123","username":"supa_agency",
        "scope":SCOPES,"expires_unix":now().unwrap()+60,"access_token":"fixture-access-token-only"});
    let c: Connection = serde_json::from_value(payload.clone()).unwrap();
    c.validate(&auth).unwrap();
    for (k, v) in [
        ("owner", json!(Uuid::from_u128(1))),
        ("client_id", json!("other")),
        ("account_id", json!("124")),
        ("scope", json!(["tweet.write"])),
        ("expires_unix", json!(1)),
        ("username", json!("other")),
        ("access_token", json!("bad\nheader")),
    ] {
        let mut p = payload.clone();
        p[k] = v;
        let c: Connection = serde_json::from_value(p).unwrap();
        assert!(c.validate(&auth).is_err());
    }
}
#[derive(Clone)]
struct Wire {
    calls: Arc<Mutex<Vec<String>>>,
    status: u16,
    mode: &'static str,
}
#[async_trait]
impl MockXTransport for Wire {
    async fn exchange(
        &self,
        path: &str,
        body: Option<Value>,
        credential: &str,
    ) -> Result<MockResponse> {
        assert_eq!(credential, "fixture-access-token-only");
        self.calls.lock().unwrap().push(path.into());
        if path == "/2/users/me" {
            if self.mode == "identity_transport" {
                return Err(Error::ProviderTransport);
            }
            return Ok(MockResponse{status:if self.mode=="identity_expired"{401}else{200},
                body:serde_json::to_vec(&json!({"data":{"id":if self.mode=="account"{"124"}else{"123"},"username":"supa_agency"}})).unwrap(),rate_limit_reset:None});
        }
        if self.mode == "disconnect" {
            return Err(Error::ProviderTransport);
        }
        Ok(MockResponse {
            status: self.status,
            body: if self.mode == "malformed" {
                b"{".to_vec()
            } else {
                serde_json::to_vec(&json!({"data":{"id":if self.mode=="bad_id"{"0"}else{"456"},"text":body.unwrap()["text"]}}))
                    .unwrap()
            },
            rate_limit_reset: Some(123456),
        })
    }
}
struct Guard(bool);
#[async_trait]
impl MockDispatchGuard for Guard {
    async fn authorize_exact(&self, _: &str, a: &MockAccount) -> Result<()> {
        assert_eq!(a.mode, "live");
        if self.0 { Ok(()) } else { Err(Error::Denied) }
    }
}
#[tokio::test]
async fn live_protocol_reuses_identity_guard_receipts_and_never_retries() {
    for (status, mode, allowed, state, calls) in [
        (201, "ok", true, "posted", 2),
        (201, "account", true, "failed", 1),
        (201, "identity_expired", true, "failed", 1),
        (201, "identity_transport", true, "failed", 1),
        (201, "ok", false, "failed", 1),
        (401, "ok", true, "failed", 2),
        (402, "ok", true, "failed", 2),
        (403, "ok", true, "failed", 2),
        (429, "ok", true, "failed", 2),
        (503, "ok", true, "uncertain", 2),
        (201, "disconnect", true, "uncertain", 2),
        (201, "malformed", true, "uncertain", 2),
        (201, "bad_id", true, "uncertain", 2),
    ] {
        let wire = Wire {
            calls: Default::default(),
            status,
            mode,
        };
        let history = wire.calls.clone();
        let adapter = MockXAdapter::prepared_live(
            wire,
            Zeroizing::new("fixture-access-token-only".into()),
            receipt().account,
            now().unwrap() + 60,
        )
        .unwrap();
        let text = "A small synthetic observation.";
        let hash = text_sha256(text);
        let outcome = adapter
            .execute_reserved_once(text, &hash, now().unwrap(), &Guard(allowed))
            .await;
        if status == 402 && mode == "ok" {
            assert_eq!(outcome.category, "insufficient_credits_no_retry");
        }
        if status == 429 && mode == "ok" {
            assert_eq!(outcome.rate_limit_reset, Some(123456));
        }
        if state == "posted" {
            assert_eq!(outcome.post_id.as_deref(), Some("456"));
            assert_eq!(outcome.category, "verified_post_receipt");
        }
        assert_eq!(outcome.mode, "live");
        assert_eq!(outcome.state, state, "{status} {mode}");
        assert_eq!(history.lock().unwrap().len(), calls);
        let retry = adapter
            .execute_reserved_once(text, &hash, now().unwrap(), &Guard(true))
            .await;
        assert_eq!(retry.category, "attempt_already_consumed");
        assert_eq!(history.lock().unwrap().len(), calls);
    }
}
#[tokio::test]
async fn expiry_and_unauthorized_operations_never_send_requests() {
    let wire = Wire {
        calls: Default::default(),
        status: 201,
        mode: "ok",
    };
    let history = wire.calls.clone();
    let adapter = MockXAdapter::prepared_live(
        wire,
        Zeroizing::new("fixture-access-token-only".into()),
        receipt().account,
        now().unwrap(),
    )
    .unwrap();
    let result = adapter
        .execute_reserved_once(
            "Synthetic.",
            &text_sha256("Synthetic."),
            now().unwrap(),
            &Guard(true),
        )
        .await;
    assert_eq!(result.state, "failed");
    assert!(history.lock().unwrap().is_empty());
    let transport = HttpsPosting::new().unwrap();
    assert!(
        transport
            .exchange("/2/dm_events", None, "fixture-access-token-only")
            .await
            .is_err()
    );
    assert!(
        transport
            .exchange(
                "/2/tweets",
                Some(json!({"text":"safe","reply":{}})),
                "fixture-access-token-only"
            )
            .await
            .is_err()
    );
    let auth = LiveAuthorization {
        receipt: receipt(),
        digest: "not-installed".into(),
    };
    assert!(
        dispatch_reserved_once(
            &auth,
            "Synthetic.",
            &text_sha256("Synthetic."),
            &Guard(true)
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn hosted_credentials_never_replace_installed_owner_authority() {
    let mut metadata = receipt();
    metadata.client_id = super::super::oauth1::HOSTED_NAMESPACE.into();
    metadata
        .validate(metadata.receipt_id, now().unwrap())
        .unwrap();
    let auth = LiveAuthorization {
        receipt: metadata,
        digest: "not-installed".into(),
    };
    assert!(verify_connection(&auth).is_err());
    struct NeverGuard;
    #[async_trait]
    impl MockDispatchGuard for NeverGuard {
        async fn check_reserved(&self, _: &str, _: &MockAccount) -> Result<()> {
            panic!("unapproved custody must not reach reservation");
        }
        async fn authorize_exact(&self, _: &str, _: &MockAccount) -> Result<()> {
            panic!("unapproved POST");
        }
    }
    assert!(
        dispatch_reserved_once(
            &auth,
            "Synthetic public result.",
            &text_sha256("Synthetic public result."),
            &NeverGuard
        )
        .await
        .is_err()
    );
}
