//! Owner-bound OAuth preparation. The local binding is inert until explicitly
//! invoked by the owner; mock adapters remain synthetic-only. No posting authority.
pub mod local;
use crate::x_review::{IDENTITY_ENDPOINT, SCOPES};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::Serialize;
use std::collections::BTreeSet;
use supa_domain::{Error, Result};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub const AUTHORIZE: &str = "https://x.com/i/oauth2/authorize";
pub const TOKEN: &str = "https://api.x.com/2/oauth2/token";
pub const REGISTERED_CALLBACK: &str = "http://127.0.0.1:37641/oauth/x/callback";
pub const CREDENTIAL_CUSTODY: &str = "supa-agency/x/supa_agency";
const MOCK_CLIENT_SECRET: &str = "synthetic-client-secret-fixture-only";
/// Implement only in the trusted backend's owner-controlled OS secret service.
/// No production implementation is installed here; tests inject a synthetic store.
/// Do not implement via browser input, dotenv, process arguments or logs.
pub trait ConfidentialCredentialStore {
    fn client_secret(&self, custody_label: &str, client_id: &str) -> Result<Zeroizing<String>>;
}
/// Secret-bearing request material has no Debug/Serialize/Clone or export API.
/// Its only consumer here is the synthetic transport test path.
pub struct MockOAuthRequest {
    method: &'static str,
    endpoint: &'static str,
    authorization: Zeroizing<String>,
    content_type: Option<&'static str>,
    body: Zeroizing<String>,
}
impl MockOAuthRequest {
    /// Non-secret routing metadata only. Credential and body getters are absent.
    pub fn method(&self) -> &'static str {
        self.method
    }
    pub fn endpoint(&self) -> &'static str {
        self.endpoint
    }
    fn validate(&self) -> Result<()> {
        let valid = match (self.method, self.endpoint) {
            ("POST", TOKEN) => {
                self.content_type == Some("application/x-www-form-urlencoded")
                    && self.authorization.starts_with("Basic ")
                    && !self.body.is_empty()
            }
            ("GET", IDENTITY_ENDPOINT) => {
                self.content_type.is_none()
                    && self.authorization.starts_with("Bearer synthetic-x-access-")
                    && self.body.is_empty()
            }
            _ => false,
        };
        if valid { Ok(()) } else { Err(Error::Denied) }
    }
}
/// Injected test responses only; never log raw bodies or raw transport errors.
pub struct MockOAuthResponse {
    pub status: u16,
    pub body: Zeroizing<Vec<u8>>,
}
#[async_trait]
pub trait MockOAuthTransport: Send + Sync {
    async fn exchange(&self, request: &MockOAuthRequest) -> Result<MockOAuthResponse>;
}
struct TokenExchange {
    form: Zeroizing<String>,
    authorization: Option<Zeroizing<String>>,
}
/// Backend-only configuration. Confidential secrets stay in zeroizing private
/// memory. A valid configuration does not establish provider registration.
pub struct OAuthPreparation {
    client_id: String,
    callback: Url,
    client_secret: Option<Zeroizing<String>>,
}
impl OAuthPreparation {
    pub fn public_client(client_id: String, registered_callback: &str) -> Result<Self> {
        let callback = Url::parse(registered_callback).map_err(|_| Error::Invalid)?;
        if client_id.is_empty()
            || client_id.len() > 200
            || !client_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || !callback.username().is_empty()
            || callback.password().is_some()
            || callback.query().is_some()
            || callback.fragment().is_some()
            || callback.path() != "/oauth/x/callback"
            || callback.as_str() != registered_callback
            || !(callback.scheme() == "https"
                && callback.host_str().is_some()
                && callback.port().is_none()
                || callback.scheme() == "http"
                    && callback.host_str() == Some("127.0.0.1")
                    && callback.port().is_some_and(|p| p >= 1024))
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            client_id,
            callback,
            client_secret: None,
        })
    }
    /// Backend-only injection from approved private custody; never dotenv, CLI,
    /// browser storage, serialization or a public request DTO. Does not load a key.
    pub fn confidential_client(
        client_id: String,
        registered_callback: &str,
        client_secret: Zeroizing<String>,
    ) -> Result<Self> {
        if registered_callback != REGISTERED_CALLBACK
            || client_secret.is_empty()
            || client_secret.len() > 4096
            || !client_secret.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::Invalid);
        }
        let mut config = Self::public_client(client_id, registered_callback)?;
        config.client_secret = Some(client_secret);
        Ok(config)
    }
    /// Validate public configuration before any private read. The future backend
    /// caller must authenticate the platform owner before using this interface.
    pub fn confidential_from_custody(
        client_id: String,
        registered_callback: &str,
        custody: &dyn ConfidentialCredentialStore,
    ) -> Result<Self> {
        if registered_callback != REGISTERED_CALLBACK {
            return Err(Error::Invalid);
        }
        Self::public_client(client_id.clone(), registered_callback)?;
        let secret = custody
            .client_secret(CREDENTIAL_CUSTODY, &client_id)
            .map_err(|_| Error::KeyVault)?;
        Self::confidential_client(client_id, registered_callback, secret)
    }
    pub fn begin(self, owner: Uuid, now: u64) -> Result<PendingConnection> {
        if owner.is_nil() {
            return Err(Error::Denied);
        }
        let mut random = [0u8; 64];
        SystemRandom::new()
            .fill(&mut random)
            .map_err(|_| Error::Invalid)?;
        let verifier = Zeroizing::new(URL_SAFE_NO_PAD.encode(&random[..32]));
        let state = URL_SAFE_NO_PAD.encode(&random[32..]);
        random.zeroize();
        let challenge =
            URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, verifier.as_bytes()).as_ref());
        let state_tag = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, verifier.as_bytes()),
            state.as_bytes(),
        )
        .as_ref()
        .to_vec();
        let mut authorize = Url::parse(AUTHORIZE).map_err(|_| Error::Invalid)?;
        authorize.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.callback.as_str()),
            ("scope", &SCOPES.join(" ")),
            ("state", &state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ]);
        Ok(PendingConnection {
            config: self,
            owner,
            created: now,
            expires: now.checked_add(600).ok_or(Error::Invalid)?,
            verifier,
            state_tag,
            authorize,
            consumed: false,
        })
    }
}
/// Deliberately no Debug/Serialize/Clone. Never print this or the callback URL.
/// A process restart discards pending state; it cannot resume/exchange a code.
pub struct PendingConnection {
    config: OAuthPreparation,
    owner: Uuid,
    created: u64,
    expires: u64,
    verifier: Zeroizing<String>,
    state_tag: Vec<u8>,
    authorize: Url,
    consumed: bool,
}
/// Sanitized proof for MOCK tests; not a credential and not a live authorization.
#[derive(Serialize)]
pub struct MockConnectionReceipt {
    pub mode: &'static str,
    pub owner_id: Uuid,
    pub account_id: String,
    pub username: String,
    pub client_id: String,
    pub expires_unix: u64,
    pub scope: Vec<String>,
    pub publishing_enabled: bool,
}
impl PendingConnection {
    /// Pure construction; does not open a browser or contact X. Return only to
    /// the bound owner after authentication permission is separately granted.
    pub fn authorize_url(&self) -> &str {
        self.authorize.as_str()
    }
    pub fn cancel(&mut self) {
        self.consumed = true;
        self.verifier.zeroize();
        self.config.client_secret.take();
    }
    fn consume(&mut self, owner: Uuid, callback: &str, now: u64) -> Result<TokenExchange> {
        if self.consumed {
            return Err(Error::Denied);
        }
        self.consumed = true; // Even malformed, denied, stale or ambiguous callbacks are one-shot.
        let parsed = (|| {
            if owner != self.owner
                || now < self.created
                || now >= self.expires
                || callback.len() > 4096
            {
                return Err(Error::Denied);
            }
            let mut url = Url::parse(callback).map_err(|_| Error::Invalid)?;
            if url.fragment().is_some() {
                return Err(Error::Denied);
            }
            let pairs: Vec<_> = url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            url.set_query(None);
            if url != self.config.callback || pairs.len() != 2 {
                return Err(Error::Denied);
            }
            let state = pairs
                .iter()
                .find(|(k, _)| k == "state")
                .map(|(_, v)| v)
                .ok_or(Error::Denied)?;
            let code = pairs
                .iter()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| v)
                .ok_or(Error::Denied)?;
            if state.len() != 43
                || code.is_empty()
                || code.len() > 2048
                || !code.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(Error::Denied);
            }
            hmac::verify(
                &hmac::Key::new(hmac::HMAC_SHA256, self.verifier.as_bytes()),
                state.as_bytes(),
                &self.state_tag,
            )
            .map_err(|_| Error::Denied)?;
            let mut form = Url::parse("https://invalid.example/").map_err(|_| Error::Invalid)?;
            form.query_pairs_mut().extend_pairs([
                ("grant_type", "authorization_code"),
                ("redirect_uri", self.config.callback.as_str()),
                ("code", code),
                ("code_verifier", &self.verifier),
            ]);
            let authorization = if let Some(secret) = self.config.client_secret.as_ref() {
                // X documents Basic base64(client_id:client_secret). Neither secret
                // nor Basic material is placed in the form, URL or owner receipt.
                let joined = Zeroizing::new(format!("{}:{}", self.config.client_id, &**secret));
                let mut header = Zeroizing::new(String::from("Basic "));
                base64::engine::general_purpose::STANDARD
                    .encode_string(joined.as_bytes(), &mut header);
                Some(header)
            } else {
                form.query_pairs_mut()
                    .append_pair("client_id", &self.config.client_id);
                None
            };
            Ok(TokenExchange {
                form: Zeroizing::new(form.query().ok_or(Error::Invalid)?.to_owned()),
                authorization,
            })
        })();
        self.verifier.zeroize();
        self.config.client_secret.take();
        parsed
    }
    /// Exercise the actual confidential request/response contract with injected
    /// synthetic responses. No real credential, authorization code, token or live
    /// transport is accepted. One consumed callback, one exchange, one GET; no retry.
    pub async fn finish_confidential_mock<T: MockOAuthTransport>(
        &mut self,
        owner: Uuid,
        callback: &str,
        now: u64,
        expected_account_id: Option<&str>,
        transport: &T,
    ) -> Result<MockConnectionReceipt> {
        let mock_secret = self
            .config
            .client_secret
            .as_ref()
            .is_some_and(|s| &**s == MOCK_CLIENT_SECRET);
        let exchange = self.consume(owner, callback, now)?;
        if !mock_secret || !synthetic_code(&exchange.form)? {
            return Err(Error::Denied);
        }
        let started = std::time::Instant::now();
        let remaining = std::time::Duration::from_secs(self.expires.saturating_sub(now))
            .min(std::time::Duration::from_secs(6));
        let result = tokio::time::timeout(remaining, async {
            let request = MockOAuthRequest {
                method: "POST",
                endpoint: TOKEN,
                authorization: exchange.authorization.ok_or(Error::Denied)?,
                content_type: Some("application/x-www-form-urlencoded"),
                body: exchange.form,
            };
            request.validate()?;
            let response = transport
                .exchange(&request)
                .await
                .map_err(|_| Error::Denied)?;
            if response.status != 200 || response.body.len() > 16384 {
                return Err(Error::Denied);
            }
            let token: TokenBody =
                serde_json::from_slice(&response.body).map_err(|_| Error::Denied)?;
            if !valid_token(&token) {
                return Err(Error::Denied);
            }
            let token_expires = now.checked_add(token.expires_in).ok_or(Error::Invalid)?;
            let identity = MockOAuthRequest {
                method: "GET",
                endpoint: IDENTITY_ENDPOINT,
                authorization: Zeroizing::new(format!("Bearer {}", &*token.access_token)),
                content_type: None,
                body: Zeroizing::new(String::new()),
            };
            identity.validate()?;
            let response = transport
                .exchange(&identity)
                .await
                .map_err(|_| Error::Denied)?;
            if response.status != 200 || response.body.len() > 16384 {
                return Err(Error::Denied);
            }
            let identity: IdentityBody =
                serde_json::from_slice(&response.body).map_err(|_| Error::Denied)?;
            let id = &identity.data.id;
            if identity.data.username != "supa_agency"
                || !id
                    .parse::<u64>()
                    .is_ok_and(|n| n > 0 && n.to_string() == *id)
                || expected_account_id.is_some_and(|expected| expected != id)
                || started.elapsed() >= remaining
                || started.elapsed() >= std::time::Duration::from_secs(token.expires_in)
            {
                return Err(Error::Denied);
            }
            Ok(MockConnectionReceipt {
                mode: "mock",
                owner_id: owner,
                account_id: identity.data.id,
                username: identity.data.username,
                client_id: self.config.client_id.clone(),
                expires_unix: token_expires,
                scope: SCOPES.iter().map(|s| s.to_string()).collect(),
                publishing_enabled: false,
            })
        })
        .await
        .map_err(|_| Error::Timeout)?;
        result
    }
    /// Consumes a synthetic callback and injected response without network access.
    /// Real codes/tokens are explicitly rejected, not silently treated as mocks.
    pub fn finish_mock(
        &mut self,
        owner: Uuid,
        callback: &str,
        now: u64,
        response: MockTokenResponse,
    ) -> Result<MockConnectionReceipt> {
        // This API remains synthetic-only even when a confidential configuration
        // was supplied. A real secret must never enter a mock transport.
        let mock_secret = self
            .config
            .client_secret
            .as_ref()
            .is_none_or(|s| &**s == MOCK_CLIENT_SECRET);
        let exchange = self.consume(owner, callback, now)?;
        if !mock_secret {
            return Err(Error::Denied);
        }
        let parsed = Url::parse(&format!("https://invalid.example/?{}", &*exchange.form))
            .map_err(|_| Error::Invalid)?;
        if !parsed
            .query_pairs()
            .any(|(k, v)| k == "code" && v.starts_with("synthetic-oauth-code-"))
        {
            return Err(Error::Denied);
        }
        let scopes: BTreeSet<_> = response.scope.split_whitespace().collect();
        if response.token_type != "bearer"
            || !response.access_token.starts_with("synthetic-x-access-")
            || response.expires_in == 0
            || response.expires_in > 7200
            || scopes != SCOPES.iter().copied().collect()
            || response.scope.split_whitespace().count() != SCOPES.len()
            || response.username != "supa_agency"
            || response.account_id.is_empty()
            || response.account_id.len() > 24
            || !response.account_id.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(Error::Denied);
        }
        Ok(MockConnectionReceipt {
            mode: "mock",
            owner_id: owner,
            account_id: response.account_id,
            username: response.username,
            client_id: self.config.client_id.clone(),
            expires_unix: now.checked_add(response.expires_in).ok_or(Error::Invalid)?,
            scope: SCOPES.iter().map(|s| s.to_string()).collect(),
            publishing_enabled: false,
        })
    }
}
fn synthetic_code(form: &str) -> Result<bool> {
    let url =
        Url::parse(&format!("https://invalid.example/?{form}")).map_err(|_| Error::Invalid)?;
    Ok(url
        .query_pairs()
        .any(|(k, v)| k == "code" && v.starts_with("synthetic-oauth-code-")))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenBody {
    #[serde(deserialize_with = "secret_string")]
    access_token: Zeroizing<String>,
    token_type: String,
    scope: String,
    expires_in: u64,
}
fn secret_string<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Zeroizing<String>, D::Error> {
    <String as serde::Deserialize>::deserialize(d).map(Zeroizing::new)
}
fn valid_token(token: &TokenBody) -> bool {
    token.access_token.starts_with("synthetic-x-access-") && valid_token_metadata(token)
}
fn valid_token_metadata(token: &TokenBody) -> bool {
    !token.access_token.is_empty()
        && token.access_token.len() <= 4096
        && token.access_token.bytes().all(|b| b.is_ascii_graphic())
        && token.token_type == "bearer"
        && (1..=7200).contains(&token.expires_in)
        && token.scope.split_whitespace().collect::<BTreeSet<_>>()
            == SCOPES.iter().copied().collect()
        && token.scope.split_whitespace().count() == SCOPES.len()
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityBody {
    data: IdentityData,
}
#[derive(serde::Deserialize)]
struct IdentityData {
    id: String,
    username: String,
}
/// Injected synthetic token + /users/me response. No raw provider response or
/// secret serialization. Token exchange and identity HTTP are NOT performed.
pub struct MockTokenResponse {
    pub access_token: Zeroizing<String>,
    pub token_type: String,
    pub scope: String,
    pub expires_in: u64,
    pub account_id: String,
    pub username: String,
}
#[cfg(test)]
mod tests {
    use super::*;
    fn prepare() -> PendingConnection {
        OAuthPreparation::public_client(
            "synthetic_public_client".into(),
            "http://127.0.0.1:37641/oauth/x/callback",
        )
        .unwrap()
        .begin(Uuid::from_u128(1), 100)
        .unwrap()
    }
    fn response() -> MockTokenResponse {
        MockTokenResponse {
            access_token: Zeroizing::new("synthetic-x-access-fixture-only".into()),
            token_type: "bearer".into(),
            scope: SCOPES.join(" "),
            expires_in: 7200,
            account_id: "123".into(),
            username: "supa_agency".into(),
        }
    }
    fn callback(p: &PendingConnection) -> String {
        let url = Url::parse(p.authorize_url()).unwrap();
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        format!(
            "http://127.0.0.1:37641/oauth/x/callback?code=synthetic-oauth-code-fixture&state={state}"
        )
    }
    #[test]
    fn s256_state_randomness_and_exact_owner_callback_are_bound() {
        let mut p = prepare();
        let other = prepare();
        assert_ne!(p.authorize_url(), other.authorize_url());
        let url = Url::parse(p.authorize_url()).unwrap();
        let q = |key: &str| {
            url.query_pairs()
                .find(|(k, _)| k == key)
                .unwrap()
                .1
                .into_owned()
        };
        assert_eq!(q("code_challenge_method"), "S256");
        assert_eq!(q("scope"), SCOPES.join(" "));
        assert!(!url.as_str().contains(&*p.verifier));
        assert_eq!(
            q("code_challenge"),
            URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, p.verifier.as_bytes()).as_ref())
        );
        let cb = callback(&p);
        let receipt = p
            .finish_mock(Uuid::from_u128(1), &cb, 101, response())
            .unwrap();
        assert!(!receipt.publishing_enabled);
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(!json.contains("synthetic-x-access") && !json.contains("synthetic-oauth-code"));
        assert!(
            p.finish_mock(Uuid::from_u128(1), &cb, 102, response())
                .is_err()
        );
    }
    #[test]
    fn callback_forgery_expiry_duplicates_denial_cancellation_are_consumed() {
        for fault in [
            "owner",
            "expiry",
            "rollback",
            "state",
            "host",
            "path",
            "duplicate",
            "denied",
            "fragment",
            "cancel",
        ] {
            let mut p = prepare();
            let mut cb = callback(&p);
            match fault {
                "state" => cb.push('x'),
                "host" => cb = cb.replace("127.0.0.1", "attacker.invalid"),
                "path" => cb = cb.replace("/oauth/x/callback", "/elsewhere"),
                "duplicate" => cb.push_str("&state=duplicate"),
                "denied" => {
                    cb = cb.replace("code=synthetic-oauth-code-fixture", "error=access_denied")
                }
                "fragment" => cb.push_str("#secret"),
                "cancel" => p.cancel(),
                _ => {}
            }
            assert!(
                p.finish_mock(
                    if fault == "owner" {
                        Uuid::from_u128(2)
                    } else {
                        Uuid::from_u128(1)
                    },
                    &cb,
                    if fault == "expiry" {
                        700
                    } else if fault == "rollback" {
                        99
                    } else {
                        101
                    },
                    response()
                )
                .is_err(),
                "{fault}"
            );
            assert!(p.consumed);
            assert!(p.verifier.is_empty());
        }
    }
    #[test]
    fn config_and_token_scope_identity_or_lifetime_mismatch_fail_closed() {
        for uri in [
            "http://example.com/oauth/x/callback",
            "http://127.0.0.1:80/oauth/x/callback",
            "https://user:your-local-password@example.com/oauth/x/callback",
            "https://example.com/oauth/x/callback?next=elsewhere",
            "https://example.com/elsewhere",
        ] {
            assert!(OAuthPreparation::public_client("client".into(), uri).is_err());
        }
        for fault in [
            "scope",
            "extra_scope",
            "duplicate_scope",
            "account",
            "token",
            "expiry",
            "type",
        ] {
            let mut p = prepare();
            let cb = callback(&p);
            let mut r = response();
            match fault {
                "scope" => r.scope = "tweet.read".into(),
                "extra_scope" => r.scope.push_str(" offline.access"),
                "duplicate_scope" => r.scope.push_str(" tweet.read"),
                "account" => r.username = "other".into(),
                "token" => r.access_token = Zeroizing::new("not-a-synthetic-token".into()),
                "expiry" => r.expires_in = 7201,
                "type" => r.token_type = "app-only".into(),
                _ => {}
            }
            assert!(
                p.finish_mock(Uuid::from_u128(1), &cb, 101, r).is_err(),
                "{fault}"
            );
        }
    }
    fn confidential() -> PendingConnection {
        OAuthPreparation::confidential_client(
            "synthetic_confidential_client".into(),
            REGISTERED_CALLBACK,
            Zeroizing::new(MOCK_CLIENT_SECRET.into()),
        )
        .unwrap()
        .begin(Uuid::from_u128(1), 100)
        .unwrap()
    }
    fn token_body() -> serde_json::Value {
        serde_json::json!({"access_token":"synthetic-x-access-fixture-only", "token_type":"bearer",
            "scope":SCOPES.join(" "),"expires_in":7200})
    }
    fn identity_body() -> serde_json::Value {
        serde_json::json!({"data":{"id":"123","username":"supa_agency","name":"Synthetic account fixture"}})
    }
    struct Fixture {
        responses: std::sync::Mutex<std::collections::VecDeque<MockOAuthResponse>>,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl Fixture {
        fn new(token: serde_json::Value, identity: serde_json::Value) -> Self {
            Self {
                responses: std::sync::Mutex::new(
                    [
                        MockOAuthResponse {
                            status: 200,
                            body: Zeroizing::new(serde_json::to_vec(&token).unwrap()),
                        },
                        MockOAuthResponse {
                            status: 200,
                            body: Zeroizing::new(serde_json::to_vec(&identity).unwrap()),
                        },
                    ]
                    .into(),
                ),
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn count(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }
    #[async_trait]
    impl MockOAuthTransport for Fixture {
        async fn exchange(&self, r: &MockOAuthRequest) -> Result<MockOAuthResponse> {
            let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if index == 0 {
                assert_eq!(r.method, "POST");
                assert_eq!(r.endpoint, TOKEN);
                assert_eq!(r.content_type, Some("application/x-www-form-urlencoded"));
                let expected = format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(format!(
                        "synthetic_confidential_client:{MOCK_CLIENT_SECRET}"
                    ))
                );
                assert_eq!(&*r.authorization, &expected);
                let form = Url::parse(&format!("https://invalid.example/?{}", &*r.body)).unwrap();
                let pairs: std::collections::BTreeMap<_, _> = form.query_pairs().collect();
                assert_eq!(pairs.len(), 4);
                assert_eq!(pairs["grant_type"], "authorization_code");
                assert_eq!(pairs["redirect_uri"], REGISTERED_CALLBACK);
                assert!(pairs["code"].starts_with("synthetic-oauth-code-"));
                assert_eq!(pairs["code_verifier"].len(), 43);
                assert!(!r.body.contains(MOCK_CLIENT_SECRET));
                assert!(!r.body.contains("client_id"));
            } else {
                assert_eq!(index, 1);
                assert_eq!(r.method, "GET");
                assert_eq!(r.endpoint, IDENTITY_ENDPOINT);
                assert_eq!(&*r.authorization, "Bearer synthetic-x-access-fixture-only");
                assert!(r.body.is_empty());
                assert_eq!(r.content_type, None);
            }
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(Error::Denied)
        }
    }
    #[tokio::test]
    async fn confidential_basic_pkce_and_read_only_identity_contract() {
        let mut p = confidential();
        let cb = callback(&p);
        let url = Url::parse(p.authorize_url()).unwrap();
        let get = |key: &str| {
            url.query_pairs()
                .find(|(k, _)| k == key)
                .unwrap()
                .1
                .into_owned()
        };
        assert_eq!(get("redirect_uri"), REGISTERED_CALLBACK);
        assert_eq!(get("code_challenge_method"), "S256");
        assert_eq!(get("scope"), SCOPES.join(" "));
        assert_eq!(
            get("code_challenge"),
            URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, p.verifier.as_bytes()).as_ref())
        );
        assert!(!p.authorize_url().contains(MOCK_CLIENT_SECRET));
        assert!(!p.authorize_url().contains(&*p.verifier));
        let fixture = Fixture::new(token_body(), identity_body());
        let receipt = p
            .finish_confidential_mock(Uuid::from_u128(1), &cb, 101, Some("123"), &fixture)
            .await
            .unwrap();
        assert_eq!(fixture.count(), 2);
        assert_eq!(receipt.account_id, "123");
        assert_eq!(receipt.username, "supa_agency");
        assert_eq!(receipt.mode, "mock");
        assert!(!receipt.publishing_enabled);
        assert_eq!(receipt.expires_unix, 7301);
        let safe = serde_json::to_string(&receipt).unwrap();
        for sensitive in [
            MOCK_CLIENT_SECRET,
            "synthetic-x-access-",
            "synthetic-oauth-code-",
            "Basic ",
            "Bearer ",
        ] {
            assert!(!safe.contains(sensitive));
        }
        assert!(p.verifier.is_empty());
        assert!(p.config.client_secret.is_none());
        assert!(
            p.finish_confidential_mock(Uuid::from_u128(1), &cb, 102, None, &fixture)
                .await
                .is_err()
        );
        assert_eq!(fixture.count(), 2);
    }
    #[tokio::test]
    async fn confidential_callbacks_consume_forgery_expiry_denial_and_cancellation_without_io() {
        for fault in [
            "owner",
            "expiry",
            "clock",
            "state",
            "host",
            "port",
            "path",
            "duplicate",
            "denied",
            "fragment",
            "cancel",
            "real_code",
        ] {
            let mut p = confidential();
            let mut cb = callback(&p);
            let fixture = Fixture::new(token_body(), identity_body());
            match fault {
                "state" => cb.push('x'),
                "host" => cb = cb.replace("127.0.0.1", "localhost"),
                "port" => cb = cb.replace(":37641", ":37642"),
                "path" => cb = cb.replace("/oauth/x/callback", "/elsewhere"),
                "duplicate" => cb.push_str("&code=duplicate"),
                "denied" => {
                    cb = cb.replace("code=synthetic-oauth-code-fixture", "error=access_denied")
                }
                "fragment" => cb.push_str("#fragment"),
                "cancel" => p.cancel(),
                "real_code" => {
                    cb = cb.replace("synthetic-oauth-code-fixture", "non-synthetic-code")
                }
                _ => {}
            }
            assert!(
                p.finish_confidential_mock(
                    if fault == "owner" {
                        Uuid::from_u128(2)
                    } else {
                        Uuid::from_u128(1)
                    },
                    &cb,
                    if fault == "expiry" {
                        700
                    } else if fault == "clock" {
                        99
                    } else {
                        101
                    },
                    None,
                    &fixture
                )
                .await
                .is_err(),
                "{fault}"
            );
            assert!(p.consumed);
            assert!(p.verifier.is_empty());
            assert!(p.config.client_secret.is_none());
            assert_eq!(fixture.count(), 0);
        }
    }
    #[tokio::test]
    async fn confidential_token_schema_scopes_refresh_and_status_fail_closed() {
        for fault in [
            "missing",
            "scope",
            "extra_scope",
            "duplicate_scope",
            "refresh",
            "expiry",
            "negative",
            "type",
            "real_token",
            "malformed",
            "oversize",
            "unauthorized",
            "rate_limit",
            "server",
        ] {
            let mut p = confidential();
            let cb = callback(&p);
            let mut token = token_body();
            match fault {
                "missing" => {
                    token.as_object_mut().unwrap().remove("access_token");
                }
                "scope" => token["scope"] = "users.read".into(),
                "extra_scope" => {
                    token["scope"] = format!("{} offline.access", SCOPES.join(" ")).into()
                }
                "duplicate_scope" => {
                    token["scope"] = format!("{} tweet.read", SCOPES.join(" ")).into()
                }
                "refresh" => token["refresh_token"] = "synthetic-refresh-not-requested".into(),
                "expiry" => token["expires_in"] = 7201.into(),
                "negative" => token["expires_in"] = (-1).into(),
                "type" => token["token_type"] = "app-only".into(),
                "real_token" => token["access_token"] = "non-synthetic-token".into(),
                _ => {}
            }
            let fixture = Fixture::new(token, identity_body());
            {
                let mut queue = fixture.responses.lock().unwrap();
                let first = queue.front_mut().unwrap();
                match fault {
                    "malformed" => first.body = Zeroizing::new(b"{".to_vec()),
                    "oversize" => first.body = Zeroizing::new(vec![b'x'; 16385]),
                    "unauthorized" => first.status = 401,
                    "rate_limit" => first.status = 429,
                    "server" => first.status = 503,
                    _ => {}
                }
            }
            assert!(
                p.finish_confidential_mock(Uuid::from_u128(1), &cb, 101, None, &fixture)
                    .await
                    .is_err(),
                "{fault}"
            );
            assert_eq!(fixture.count(), 1);
            assert!(
                p.finish_confidential_mock(Uuid::from_u128(1), &cb, 102, None, &fixture)
                    .await
                    .is_err()
            );
            assert_eq!(fixture.count(), 1);
        }
    }
    #[tokio::test]
    async fn confidential_identity_mismatch_and_partial_error_never_connect() {
        for fault in [
            "username",
            "id",
            "zero",
            "leading_zero",
            "overflow",
            "missing",
            "partial_error",
            "pinned",
            "malformed",
            "status",
            "oversize",
        ] {
            let mut p = confidential();
            let cb = callback(&p);
            let mut identity = identity_body();
            match fault {
                "username" => identity["data"]["username"] = "another_account".into(),
                "id" => identity["data"]["id"] = "abc".into(),
                "zero" => identity["data"]["id"] = "0".into(),
                "leading_zero" => identity["data"]["id"] = "0123".into(),
                "overflow" => identity["data"]["id"] = "18446744073709551616".into(),
                "missing" => {
                    identity["data"].as_object_mut().unwrap().remove("id");
                }
                "partial_error" => {
                    identity["errors"] = serde_json::json!([{"message":"synthetic partial error"}])
                }
                _ => {}
            }
            let fixture = Fixture::new(token_body(), identity);
            {
                let mut queue = fixture.responses.lock().unwrap();
                let last = queue.back_mut().unwrap();
                match fault {
                    "malformed" => last.body = Zeroizing::new(b"{".to_vec()),
                    "status" => last.status = 403,
                    "oversize" => last.body = Zeroizing::new(vec![b'x'; 16385]),
                    _ => {}
                }
            }
            assert!(
                p.finish_confidential_mock(
                    Uuid::from_u128(1),
                    &cb,
                    101,
                    if fault == "pinned" { Some("456") } else { None },
                    &fixture
                )
                .await
                .is_err(),
                "{fault}"
            );
            assert_eq!(fixture.count(), 2);
        }
    }
    #[test]
    fn confidential_configuration_rejects_callback_changes_and_invalid_secrets() {
        for callback in [
            "http://localhost:37641/oauth/x/callback",
            "http://127.0.0.1:37642/oauth/x/callback",
            "https://example.com/oauth/x/callback",
        ] {
            assert!(
                OAuthPreparation::confidential_client(
                    "client".into(),
                    callback,
                    Zeroizing::new(MOCK_CLIENT_SECRET.into())
                )
                .is_err()
            );
        }
        for secret in [
            String::new(),
            " secret".into(),
            "secret\n".into(),
            "sëcret".into(),
            "x".repeat(4097),
        ] {
            assert!(
                OAuthPreparation::confidential_client(
                    "client".into(),
                    REGISTERED_CALLBACK,
                    Zeroizing::new(secret)
                )
                .is_err()
            );
        }
        assert!(
            OAuthPreparation::confidential_client(
                String::new(),
                REGISTERED_CALLBACK,
                Zeroizing::new(MOCK_CLIENT_SECRET.into())
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn confidential_mock_refuses_non_fixture_secret_without_transmission() {
        let mut p = OAuthPreparation::confidential_client(
            "client".into(),
            REGISTERED_CALLBACK,
            Zeroizing::new("different-synthetic-secret".into()),
        )
        .unwrap()
        .begin(Uuid::from_u128(1), 100)
        .unwrap();
        let cb = callback(&p);
        let fixture = Fixture::new(token_body(), identity_body());
        assert!(
            p.finish_confidential_mock(Uuid::from_u128(1), &cb, 101, None, &fixture)
                .await
                .is_err()
        );
        assert_eq!(fixture.count(), 0);
        assert!(p.config.client_secret.is_none());
        let mut public = prepare();
        let cb = callback(&public);
        assert!(
            public
                .finish_confidential_mock(Uuid::from_u128(1), &cb, 101, None, &fixture)
                .await
                .is_err()
        );
        assert_eq!(fixture.count(), 0);
    }
    struct Interrupted {
        calls: std::sync::atomic::AtomicUsize,
        delay: bool,
    }
    #[async_trait]
    impl MockOAuthTransport for Interrupted {
        async fn exchange(&self, _: &MockOAuthRequest) -> Result<MockOAuthResponse> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.delay {
                tokio::time::sleep(std::time::Duration::from_secs(7)).await;
            }
            Err(Error::ProviderTransport)
        }
    }
    #[tokio::test]
    async fn confidential_transport_failure_timeout_and_future_cancellation_are_one_shot() {
        for fault in ["network", "timeout", "cancel_future"] {
            let mut p = confidential();
            let cb = callback(&p);
            let fixture = Interrupted {
                calls: std::sync::atomic::AtomicUsize::new(0),
                delay: fault != "network",
            };
            if fault == "cancel_future" {
                let mut future = Box::pin(p.finish_confidential_mock(
                    Uuid::from_u128(1),
                    &cb,
                    101,
                    None,
                    &fixture,
                ));
                tokio::select! {_=&mut future=>panic!("unexpected completion"),_=tokio::time::sleep(std::time::Duration::from_millis(20))=>{}}
                drop(future);
            } else {
                assert!(
                    p.finish_confidential_mock(Uuid::from_u128(1), &cb, 101, None, &fixture)
                        .await
                        .is_err()
                );
            }
            assert_eq!(fixture.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert!(p.consumed);
            assert!(p.verifier.is_empty());
            assert!(p.config.client_secret.is_none());
            assert!(
                p.finish_confidential_mock(Uuid::from_u128(1), &cb, 102, None, &fixture)
                    .await
                    .is_err()
            );
            assert_eq!(fixture.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
    }
    struct DelayedIdentity(Fixture);
    #[async_trait]
    impl MockOAuthTransport for DelayedIdentity {
        async fn exchange(&self, r: &MockOAuthRequest) -> Result<MockOAuthResponse> {
            if r.method() == "GET" {
                tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
            }
            self.0.exchange(r).await
        }
    }
    #[tokio::test]
    async fn confidential_callback_and_token_expiry_include_transport_time() {
        let mut p = confidential();
        let cb = callback(&p);
        let mut token = token_body();
        token["expires_in"] = 1.into();
        let fixture = DelayedIdentity(Fixture::new(token, identity_body()));
        assert!(
            p.finish_confidential_mock(Uuid::from_u128(1), &cb, 101, None, &fixture)
                .await
                .is_err()
        );
        assert_eq!(fixture.0.count(), 2);
        let mut p = confidential();
        let cb = callback(&p);
        let fixture = Interrupted {
            calls: std::sync::atomic::AtomicUsize::new(0),
            delay: true,
        };
        assert!(
            p.finish_confidential_mock(Uuid::from_u128(1), &cb, 699, None, &fixture)
                .await
                .is_err()
        );
        assert_eq!(fixture.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(p.consumed);
    }
    #[test]
    fn confidential_form_escapes_code_and_never_places_secret_in_body_or_url() {
        let secret = "synthetic-confidential-fixture:+/=";
        let mut p = OAuthPreparation::confidential_client(
            "client".into(),
            REGISTERED_CALLBACK,
            Zeroizing::new(secret.into()),
        )
        .unwrap()
        .begin(Uuid::from_u128(1), 100)
        .unwrap();
        let cb = callback(&p).replace(
            "synthetic-oauth-code-fixture",
            "synthetic-oauth-code-fixture%2B%26%3D",
        );
        let exchange = p.consume(Uuid::from_u128(1), &cb, 101).unwrap();
        let url = Url::parse(&format!("https://invalid.example/?{}", &*exchange.form)).unwrap();
        assert_eq!(
            url.query_pairs().find(|(k, _)| k == "code").unwrap().1,
            "synthetic-oauth-code-fixture+&="
        );
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(
                exchange
                    .authorization
                    .as_ref()
                    .unwrap()
                    .strip_prefix("Basic ")
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(decoded, format!("client:{secret}").into_bytes());
        assert!(!exchange.form.contains(secret));
        assert!(!p.authorize_url().contains(secret));
        assert!(p.config.client_secret.is_none());
    }
    #[test]
    fn confidential_private_custody_is_scoped_and_missing_storage_fails_closed() {
        struct Store {
            calls: std::cell::Cell<usize>,
            unavailable: bool,
        }
        impl ConfidentialCredentialStore for Store {
            fn client_secret(&self, label: &str, client: &str) -> Result<Zeroizing<String>> {
                self.calls.set(self.calls.get() + 1);
                assert_eq!(label, CREDENTIAL_CUSTODY);
                assert_eq!(client, "synthetic_confidential_client");
                if self.unavailable {
                    Err(Error::KeyVault)
                } else {
                    Ok(Zeroizing::new(MOCK_CLIENT_SECRET.into()))
                }
            }
        }
        let store = Store {
            calls: std::cell::Cell::new(0),
            unavailable: false,
        };
        for (client, callback) in [
            (
                "synthetic_confidential_client",
                "http://localhost:37641/oauth/x/callback",
            ),
            ("", REGISTERED_CALLBACK),
        ] {
            assert!(
                OAuthPreparation::confidential_from_custody(client.into(), callback, &store)
                    .is_err()
            );
        }
        assert_eq!(store.calls.get(), 0);
        let config = OAuthPreparation::confidential_from_custody(
            "synthetic_confidential_client".into(),
            REGISTERED_CALLBACK,
            &store,
        )
        .unwrap();
        let pending = config.begin(Uuid::from_u128(1), 100).unwrap();
        assert!(!pending.authorize_url().contains(MOCK_CLIENT_SECRET));
        assert_eq!(store.calls.get(), 1);
        let absent = Store {
            calls: std::cell::Cell::new(0),
            unavailable: true,
        };
        assert!(
            OAuthPreparation::confidential_from_custody(
                "synthetic_confidential_client".into(),
                REGISTERED_CALLBACK,
                &absent
            )
            .is_err()
        );
        assert_eq!(absent.calls.get(), 1);
    }
}
