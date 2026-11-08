//! X v2 protocol preparation. Production authentication/posting is unavailable.
//! Only injected mock transports are used; no environment credential lookup.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use supa_domain::{Error, Result, social_review::safe_post};
use zeroize::Zeroizing;

pub const IDENTITY_ENDPOINT: &str = "https://api.x.com/2/users/me";
pub const POST_ENDPOINT: &str = "https://api.x.com/2/tweets";
pub const SCOPES: &[&str] = &["tweet.read", "tweet.write", "users.read"];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MockAccount {
    pub id: String,
    pub username: String,
    pub mode: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XReceipt {
    pub mode: String,
    pub state: String,
    pub category: String,
    pub post_id: Option<String>,
    pub http_status: Option<u16>,
    pub rate_limit_reset: Option<u64>,
}
impl XReceipt {
    fn outcome(state: &str, category: &str) -> Self {
        Self {
            mode: "mock".into(),
            state: state.into(),
            category: category.into(),
            post_id: None,
            http_status: None,
            rate_limit_reset: None,
        }
    }
}
/// Mock responses only. Never retain/log a raw body or request credentials.
pub struct MockResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub rate_limit_reset: Option<u64>,
}
#[async_trait]
pub trait MockXTransport: Send + Sync {
    async fn exchange(
        &self,
        path: &str,
        body: Option<Value>,
        credential: &str,
    ) -> Result<MockResponse>;
}
#[async_trait]
pub trait MockDispatchGuard: Send + Sync {
    async fn authorize_exact(&self, text_sha256: &str, account: &MockAccount) -> Result<()>;
}
/// Owns a secret only in local mock tests; deliberately no Debug/Serialize/Clone.
pub struct MockXAdapter<T: MockXTransport> {
    transport: T,
    credential: Zeroizing<String>,
    account: MockAccount,
    expires_unix: u64,
    attempted: AtomicBool,
}
impl<T: MockXTransport> MockXAdapter<T> {
    pub fn new(
        transport: T,
        credential: String,
        account: MockAccount,
        expires_unix: u64,
    ) -> Result<Self> {
        if account.mode != "mock"
            || account.username != "supa_agency"
            || account.id.is_empty()
            || account.id.len() > 24
            || !account.id.bytes().all(|b| b.is_ascii_digit())
            || credential != "mock-x-credential-no-real-account"
        {
            return Err(Error::Denied);
        }
        Ok(Self {
            transport,
            credential: Zeroizing::new(credential),
            account,
            expires_unix,
            attempted: AtomicBool::new(false),
        })
    }
    pub async fn execute_reserved_once(
        &self,
        text: &str,
        text_sha256: &str,
        now_unix: u64,
        guard: &dyn MockDispatchGuard,
    ) -> XReceipt {
        if self.attempted.swap(true, Ordering::SeqCst) {
            return XReceipt::outcome("failed", "attempt_already_consumed");
        }
        let hash = ring::digest::digest(&ring::digest::SHA256, text.as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if safe_post(text).is_err() || hash != text_sha256 || now_unix >= self.expires_unix {
            return XReceipt::outcome("failed", "invalid_text_or_expired_authorization");
        }
        let identity = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.transport
                .exchange("/2/users/me", None, &self.credential),
        )
        .await
        {
            Ok(Ok(r)) => r,
            _ => return XReceipt::outcome("failed", "identity_unavailable"),
        };
        if identity.status != 200 || identity.body.len() > 16384 {
            return XReceipt::outcome("failed", "identity_unavailable");
        }
        let identity: Value = match serde_json::from_slice(&identity.body) {
            Ok(v) => v,
            Err(_) => return XReceipt::outcome("failed", "identity_invalid"),
        };
        if identity["data"]["id"] != self.account.id
            || identity["data"]["username"] != self.account.username
        {
            return XReceipt::outcome("failed", "account_mismatch");
        }
        if guard
            .authorize_exact(text_sha256, &self.account)
            .await
            .is_err()
        {
            return XReceipt::outcome("failed", "approval_revoked");
        }
        let response = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.transport
                .exchange("/2/tweets", Some(json!({"text":text})), &self.credential),
        )
        .await
        {
            Ok(Ok(r)) => r,
            _ => return XReceipt::outcome("uncertain", "ambiguous_transport_no_retry"),
        };
        let mut receipt = XReceipt::outcome("uncertain", "unverified_post_response");
        receipt.http_status = Some(response.status);
        receipt.rate_limit_reset = response.rate_limit_reset;
        match response.status {
            200..=299 if response.body.len() <= 16384 => {
                if let Ok(value) = serde_json::from_slice::<Value>(&response.body) {
                    if let Some(id) = value["data"]["id"].as_str() {
                        if !id.is_empty()
                            && id.len() <= 24
                            && id.bytes().all(|b| b.is_ascii_digit())
                        {
                            receipt.state = "posted".into();
                            receipt.category = "mock_post_receipt".into();
                            receipt.post_id = Some(id.into());
                        }
                    }
                }
            }
            400 | 401 | 403 | 404 | 422 | 429 => {
                receipt.state = "failed".into();
                receipt.category = if response.status == 429 {
                    "rate_limited_no_retry"
                } else {
                    "request_rejected_no_retry"
                }
                .into();
            }
            _ => {}
        }
        receipt
    }
}
/// There is deliberately no live client constructor or credential loader.
pub fn production_posting() -> Result<()> {
    Err(Error::Denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Wire {
        fault: &'static str,
        calls: Arc<Mutex<Vec<String>>>,
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
                return Ok(MockResponse{status:200,body:json!({"data":{"id":if self.fault=="account"{"2"}else{"1"},"username":"supa_agency"}}).to_string().into_bytes(),rate_limit_reset:None});
            }
            assert_eq!(path, "/2/tweets");
            assert_eq!(body.unwrap(), json!({"text":"A bounded observation."}));
            if self.fault == "transport" {
                return Err(Error::ProviderTransport);
            }
            Ok(MockResponse {
                status: match self.fault {
                    "rate" => 429,
                    "http" => 503,
                    _ => 201,
                },
                body: if self.fault == "json" {
                    b"private raw response".to_vec()
                } else {
                    br#"{"data":{"id":"123"}}"#.to_vec()
                },
                rate_limit_reset: Some(123456),
            })
        }
    }
    struct Guard(bool);
    #[async_trait]
    impl MockDispatchGuard for Guard {
        async fn authorize_exact(&self, _: &str, _: &MockAccount) -> Result<()> {
            if self.0 { Ok(()) } else { Err(Error::Denied) }
        }
    }
    #[tokio::test]
    async fn mock_protocol_is_one_shot_and_uncertainty_never_replays() {
        for fault in [
            "none",
            "account",
            "expired",
            "revoked",
            "transport",
            "http",
            "json",
            "rate",
        ] {
            let calls = Arc::new(Mutex::new(vec![]));
            let adapter = MockXAdapter::new(
                Wire {
                    fault,
                    calls: calls.clone(),
                },
                "mock-x-credential-no-real-account".into(),
                MockAccount {
                    id: "1".into(),
                    username: "supa_agency".into(),
                    mode: "mock".into(),
                },
                100,
            )
            .unwrap();
            let text = "A bounded observation.";
            let hash = ring::digest::digest(&ring::digest::SHA256, text.as_bytes())
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let receipt = adapter
                .execute_reserved_once(
                    text,
                    &hash,
                    if fault == "expired" { 100 } else { 1 },
                    &Guard(fault != "revoked"),
                )
                .await;
            assert_eq!(
                receipt.state,
                match fault {
                    "none" => "posted",
                    "transport" | "http" | "json" => "uncertain",
                    _ => "failed",
                }
            );
            let count = calls.lock().unwrap().len();
            assert_eq!(
                count,
                if fault == "expired" {
                    0
                } else if matches!(fault, "account" | "revoked") {
                    1
                } else {
                    2
                }
            );
            assert_eq!(
                adapter
                    .execute_reserved_once(text, &hash, 1, &Guard(true))
                    .await
                    .category,
                "attempt_already_consumed"
            );
            assert_eq!(calls.lock().unwrap().len(), count);
            assert!(
                !serde_json::to_string(&receipt)
                    .unwrap()
                    .contains("private raw response")
            );
        }
        assert!(production_posting().is_err());
    }
    #[tokio::test]
    async fn loopback_http_contract_never_contacts_x() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for path in ["/2/users/me", "/2/tweets"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut raw = vec![0u8; 8192];
                let mut size = 0;
                loop {
                    let n = socket.read(&mut raw[size..]).await.unwrap();
                    assert!(n > 0);
                    size += n;
                    let text = String::from_utf8_lossy(&raw[..size]);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if size >= end + 4 + length {
                            break;
                        }
                    }
                    assert!(size < 8192);
                }
                let raw = String::from_utf8(raw[..size].to_vec()).unwrap();
                assert!(raw.starts_with(&format!(
                    "{} {path} HTTP/1.1",
                    if path == "/2/tweets" { "POST" } else { "GET" }
                )));
                assert!(
                    raw.to_lowercase()
                        .contains("authorization: bearer mock-x-credential-no-real-account")
                );
                let body = if path == "/2/users/me" {
                    r#"{"data":{"id":"1","username":"supa_agency"}}"#
                } else {
                    assert!(raw.ends_with(r#"{"text":"A bounded observation."}"#));
                    r#"{"data":{"id":"123"}}"#
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        struct Local(reqwest::Client, String);
        #[async_trait]
        impl MockXTransport for Local {
            async fn exchange(
                &self,
                path: &str,
                body: Option<Value>,
                credential: &str,
            ) -> Result<MockResponse> {
                let mut r = self
                    .0
                    .request(
                        if body.is_some() {
                            reqwest::Method::POST
                        } else {
                            reqwest::Method::GET
                        },
                        format!("{}{path}", self.1),
                    )
                    .bearer_auth(credential);
                if let Some(b) = body {
                    r = r.json(&b);
                }
                let r = r.send().await.map_err(|_| Error::ProviderTransport)?;
                Ok(MockResponse {
                    status: r.status().as_u16(),
                    body: r
                        .bytes()
                        .await
                        .map_err(|_| Error::ProviderTransport)?
                        .to_vec(),
                    rate_limit_reset: None,
                })
            }
        }
        let adapter = MockXAdapter::new(
            Local(
                reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                format!("http://{address}"),
            ),
            "mock-x-credential-no-real-account".into(),
            MockAccount {
                id: "1".into(),
                username: "supa_agency".into(),
                mode: "mock".into(),
            },
            100,
        )
        .unwrap();
        let text = "A bounded observation.";
        let hash = ring::digest::digest(&ring::digest::SHA256, text.as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(
            adapter
                .execute_reserved_once(text, &hash, 1, &Guard(true))
                .await
                .state,
            "posted"
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn timed_out_post_is_uncertain_and_consumed() {
        struct Slow;
        #[async_trait]
        impl MockXTransport for Slow {
            async fn exchange(
                &self,
                path: &str,
                _: Option<Value>,
                _: &str,
            ) -> Result<MockResponse> {
                if path == "/2/users/me" {
                    return Ok(MockResponse {
                        status: 200,
                        body: br#"{"data":{"id":"1","username":"supa_agency"}}"#.to_vec(),
                        rate_limit_reset: None,
                    });
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                unreachable!("bounded adapter must cancel this mock transport")
            }
        }
        let a = MockXAdapter::new(
            Slow,
            "mock-x-credential-no-real-account".into(),
            MockAccount {
                id: "1".into(),
                username: "supa_agency".into(),
                mode: "mock".into(),
            },
            100,
        )
        .unwrap();
        let text = "A bounded observation.";
        let hash = ring::digest::digest(&ring::digest::SHA256, text.as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let start = std::time::Instant::now();
        assert_eq!(
            a.execute_reserved_once(text, &hash, 1, &Guard(true))
                .await
                .state,
            "uncertain"
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(
            a.execute_reserved_once(text, &hash, 1, &Guard(true))
                .await
                .category,
            "attempt_already_consumed"
        );
    }
}
