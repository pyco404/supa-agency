//! OAuth 1.0a identity and explicitly reserved text-only posting. No activation or retries.
use super::{
    IDENTITY_ENDPOINT, MockAccount, MockDispatchGuard, MockResponse, MockXAdapter, MockXTransport,
    POST_ENDPOINT, XReceipt, live::numeric_id,
};
use crate::x_oauth::local::{PrivateKeyring, authorization::AuthorizationReceipt};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use supa_domain::social_review::safe_post;
use supa_domain::{Error, Result};
use zeroize::Zeroizing;

pub const NAMESPACE: &str = "oauth1-supa-agency";
/// Only an independently installed autonomous authorization can use this custody.
pub const HOSTED_NAMESPACE: &str = "oauth1-supa-agency-hosted";
const HOSTED_CREDENTIAL: &str = "/run/credentials/supa-x-autonomy.service/oauth1.json";
fn service_credentials(path: &std::path::Path) -> Result<Credentials> {
    use std::{io::Read, os::unix::fs::MetadataExt};
    let uid = std::fs::metadata("/proc/self")
        .map_err(|_| Error::KeyVault)?
        .uid();
    if uid == 0 {
        return Err(Error::Denied);
    }
    for parent in path.ancestors().skip(1) {
        let m = std::fs::symlink_metadata(parent).map_err(|_| Error::KeyVault)?;
        if !m.is_dir()
            || m.file_type().is_symlink()
            || m.mode() & 0o022 != 0
            || (m.uid() != 0 && m.uid() != uid)
        {
            return Err(Error::KeyVault);
        }
    }
    let before = std::fs::symlink_metadata(path).map_err(|_| Error::KeyVault)?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.uid() != uid
        || before.mode() & 0o077 != 0
        || before.nlink() != 1
        || before.len() > 16384
    {
        return Err(Error::KeyVault);
    }
    let file = std::fs::File::open(path).map_err(|_| Error::KeyVault)?;
    let after = file.metadata().map_err(|_| Error::KeyVault)?;
    if before.ino() != after.ino()
        || before.dev() != after.dev()
        || after.uid() != uid
        || after.mode() & 0o077 != 0
        || after.nlink() != 1
        || after.len() > 16384
    {
        return Err(Error::KeyVault);
    }
    let mut raw = Zeroizing::new(Vec::new());
    file.take(16385)
        .read_to_end(&mut raw)
        .map_err(|_| Error::KeyVault)?;
    if raw.len() > 16384 {
        return Err(Error::KeyVault);
    }
    let credentials: Credentials = serde_json::from_slice(&raw).map_err(|_| Error::KeyVault)?;
    credentials.validate()?;
    Ok(credentials)
}
fn hosted_credentials(auth: &super::live::LiveAuthorization) -> Result<Credentials> {
    auth.check()?;
    if auth.client_id() != HOSTED_NAMESPACE {
        return Err(Error::Denied);
    }
    let credentials = service_credentials(std::path::Path::new(HOSTED_CREDENTIAL))?;
    auth.check()?;
    Ok(credentials)
}
pub(super) fn hosted_credentials_available(auth: &super::live::LiveAuthorization) -> Result<()> {
    hosted_credentials(auth).map(|_| ())
}
pub(super) async fn dispatch_hosted(
    auth: &super::live::LiveAuthorization,
    text: &str,
    hash: &str,
    guard: &dyn MockDispatchGuard,
) -> Result<XReceipt> {
    let post = ReservedPost {
        account: auth.account().clone(),
        text,
        text_sha256: hash,
        expires_unix: auth.expires_unix().min(now()?.saturating_add(600)),
        budget_microusd: 25000,
    };
    authorize_reservation(&post, guard).await?;
    let credentials = hosted_credentials(auth)?;
    let identity = IdentityClient::new()?;
    let wire = PostingTransport {
        client: identity.client,
        credentials,
        identity_url: identity.endpoint,
        post_url: POST_ENDPOINT.into(),
        session: None,
        live_authorization: Some(auth),
        expires: post.expires_unix,
    };
    Ok(MockXAdapter::prepared_live(
        wire,
        Zeroizing::new(String::new()),
        post.account,
        post.expires_unix,
    )?
    .execute_reserved_once(text, hash, now()?, guard)
    .await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    #[serde(rename = "X_API_KEY", deserialize_with = "secret")]
    api_key: Zeroizing<String>,
    #[serde(rename = "X_API_SECRET", deserialize_with = "secret")]
    api_secret: Zeroizing<String>,
    #[serde(rename = "X_ACCESS_TOKEN", deserialize_with = "secret")]
    access_token: Zeroizing<String>,
    #[serde(rename = "X_ACCESS_SECRET", deserialize_with = "secret")]
    access_secret: Zeroizing<String>,
}
fn secret<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Zeroizing<String>, D::Error> {
    String::deserialize(d).map(Zeroizing::new)
}
fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::Denied)
}
fn encode(s: &str) -> String {
    let mut result = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            result.push(b as char)
        } else {
            result.push_str(&format!("%{b:02X}"));
        }
    }
    result
}
impl Credentials {
    fn validate(&self) -> Result<()> {
        for s in [
            &self.api_key,
            &self.api_secret,
            &self.access_token,
            &self.access_secret,
        ] {
            if s.is_empty() || s.len() > 1024 || !s.bytes().all(|b| (33..=126).contains(&b)) {
                return Err(Error::KeyVault);
            }
        }
        Ok(())
    }
    fn header(&self, method: &str, url: &str, nonce: &str, at: u64) -> Result<Zeroizing<String>> {
        self.validate()?;
        if !matches!(method, "GET" | "POST") {
            return Err(Error::Denied);
        }
        let mut params: Vec<(&str, &str)> = vec![
            ("oauth_consumer_key", &*self.api_key),
            ("oauth_nonce", nonce),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", ""),
            ("oauth_token", &*self.access_token),
            ("oauth_version", "1.0"),
        ];
        let timestamp = at.to_string();
        params[3].1 = &timestamp;
        let normalized = Zeroizing::new(
            params
                .iter()
                .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
                .collect::<Vec<_>>()
                .join("&"),
        );
        let base = Zeroizing::new(format!("{method}&{}&{}", encode(url), encode(&normalized)));
        let signing_key = Zeroizing::new(format!(
            "{}&{}",
            encode(&self.api_secret),
            encode(&self.access_secret)
        ));
        // HMAC-SHA1 is the mandated OAuth 1.0a signature, not a password hash.
        let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, signing_key.as_bytes());
        let signature = STANDARD.encode(hmac::sign(&key, base.as_bytes()));
        let mut fields = params
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", encode(k), encode(v)))
            .collect::<Vec<_>>();
        fields.push(format!("oauth_signature=\"{}\"", encode(&signature)));
        fields.sort();
        Ok(Zeroizing::new(format!("OAuth {}", fields.join(", "))))
    }
}
#[derive(Serialize)]
pub struct IdentityReceipt {
    pub auth_mode: &'static str,
    pub account_id: String,
    pub username: String,
    pub verified_at: u64,
    pub publishing_enabled: bool,
}
struct IdentityClient {
    client: reqwest::Client,
    endpoint: String,
}
impl IdentityClient {
    fn new() -> Result<Self> {
        Ok(Self {
            endpoint: IDENTITY_ENDPOINT.into(),
            client: reqwest::Client::builder()
                .https_only(true)
                .min_tls_version(reqwest::tls::Version::TLS_1_2)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(3))
                .build()
                .map_err(|_| Error::ProviderTransport)?,
        })
    }
    async fn once(&self, credentials: &Credentials) -> Result<IdentityReceipt> {
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Denied)?;
        let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let auth = credentials.header("GET", &self.endpoint, &nonce, now()?)?;
        let mut header =
            reqwest::header::HeaderValue::from_str(&auth).map_err(|_| Error::Denied)?;
        header.set_sensitive(true);
        let mut response = self
            .client
            .get(&self.endpoint)
            .header(reqwest::header::AUTHORIZATION, header)
            .send()
            .await
            .map_err(|_| Error::ProviderTransport)?;
        if response.status().as_u16() != 200 {
            return Err(Error::ProviderHttp(response.status().as_u16()));
        }
        if response.content_length().is_some_and(|n| n > 16384)
            || !response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.split(';').next() == Some("application/json"))
        {
            return Err(Error::Provider);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::ProviderTransport)?
        {
            if bytes.len() + chunk.len() > 16384 {
                return Err(Error::Provider);
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| Error::Provider)?;
        let id = value["data"]["id"]
            .as_str()
            .filter(|v| numeric_id(v))
            .ok_or(Error::Denied)?;
        if value.get("errors").is_some() || value["data"]["username"] != "supa_agency" {
            return Err(Error::Denied);
        }
        Ok(IdentityReceipt {
            auth_mode: "oauth1",
            account_id: id.into(),
            username: "supa_agency".into(),
            verified_at: now()?,
            publishing_enabled: false,
        })
    }
}
/// Exactly one budgeted GET; never called by setup/status. No automatic retry.
/// No API key, secret, access token or signed header can leave backend custody.
pub async fn verify_official_account(
    authorization: AuthorizationReceipt,
) -> Result<IdentityReceipt> {
    let owner = "6fd863e1-745d-4a06-b68e-eea69e854a69"
        .parse()
        .map_err(|_| Error::Denied)?;
    if !authorization.matches(owner, NAMESPACE) {
        return Err(Error::Denied);
    }
    authorization.check()?;
    let bytes = PrivateKeyring::new(owner)?.read_oauth1_env()?;
    let credentials: Credentials = serde_json::from_slice(&bytes).map_err(|_| Error::KeyVault)?;
    credentials.validate()?;
    let client = IdentityClient::new()?;
    authorization.check()?;
    let receipt = client.once(&credentials).await?;
    authorization.check()?;
    Ok(receipt)
}
/// Exact caller-approved reservation. This is data, never authority by itself.
/// The required durable guard must bind account/text, owner, expiry and budget,
/// consume one attempt, and preserve uncertain-account quarantine across restarts.
pub struct ReservedPost<'a> {
    pub account: MockAccount,
    pub text: &'a str,
    pub text_sha256: &'a str,
    pub expires_unix: u64,
    pub budget_microusd: u64,
}
impl ReservedPost<'_> {
    fn validate(&self, at: u64) -> Result<()> {
        safe_post(self.text)?;
        if self.account.mode != "live"
            || self.account.username != "supa_agency"
            || !numeric_id(&self.account.id)
            || super::text_sha256(self.text) != self.text_sha256
            || self.expires_unix <= at
            || self.expires_unix > at.saturating_add(600)
            || self.budget_microusd != 25000
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
struct PostingTransport<'a> {
    client: reqwest::Client,
    credentials: Credentials,
    identity_url: String,
    post_url: String,
    session: Option<&'a AuthorizationReceipt>,
    live_authorization: Option<&'a super::live::LiveAuthorization>,
    expires: u64,
}
#[async_trait]
impl MockXTransport for PostingTransport<'_> {
    async fn exchange(&self, path: &str, body: Option<Value>, _: &str) -> Result<MockResponse> {
        if now()? >= self.expires {
            return Err(Error::Denied);
        }
        if let Some(session) = self.session {
            session.check()?;
        }
        if let Some(live) = self.live_authorization {
            live.check()?;
        }
        // A session observation cannot extend the post reservation deadline.
        if now()? >= self.expires {
            return Err(Error::Denied);
        }
        let (method, endpoint, request) = match (path, body) {
            ("/2/users/me", None) => (
                "GET",
                &self.identity_url,
                self.client.get(&self.identity_url),
            ),
            ("/2/tweets", Some(body))
                if body
                    .as_object()
                    .is_some_and(|m| m.len() == 1 && m.contains_key("text")) =>
            {
                safe_post(body["text"].as_str().ok_or(Error::Denied)?)?;
                // JSON body fields are not OAuth form parameters. TLS and the exact-text guard protect the body.
                (
                    "POST",
                    &self.post_url,
                    self.client.post(&self.post_url).json(&body),
                )
            }
            _ => return Err(Error::Denied),
        };
        let mut nonce = [0; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Denied)?;
        let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let auth = self.credentials.header(method, endpoint, &nonce, now()?)?;
        let mut header =
            reqwest::header::HeaderValue::from_str(&auth).map_err(|_| Error::Denied)?;
        header.set_sensitive(true);
        let mut response = request
            .header(reqwest::header::AUTHORIZATION, header)
            .send()
            .await
            .map_err(|_| Error::ProviderTransport)?;
        let status = response.status().as_u16();
        let rate_limit_reset = response
            .headers()
            .get("x-rate-limit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        if !(200..300).contains(&status) {
            return Ok(MockResponse {
                status,
                body: vec![],
                rate_limit_reset,
            });
        }
        if response.content_length().is_some_and(|n| n > 16384)
            || !response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.split(';').next() == Some("application/json"))
        {
            return Err(Error::Provider);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::ProviderTransport)?
        {
            if bytes.len() + chunk.len() > 16384 {
                return Err(Error::Provider);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(MockResponse {
            status,
            body: bytes.to_vec(),
            rate_limit_reset,
        })
    }
}
async fn authorize_reservation(
    post: &ReservedPost<'_>,
    guard: &dyn MockDispatchGuard,
) -> Result<()> {
    post.validate(now()?)?;
    tokio::time::timeout(
        Duration::from_secs(1),
        guard.check_reserved(post.text_sha256, &post.account),
    )
    .await
    .map_err(|_| Error::Denied)??;
    Ok(())
}
/// No connection or environment flag grants posting authority. Caller MUST reserve
/// durably first and settle the returned receipt (or quarantine interrupted work).
/// Fixed price reservation covers one identity read + one text-only create.
pub async fn dispatch_reserved_once(
    authorization: &AuthorizationReceipt,
    post: &ReservedPost<'_>,
    guard: &dyn MockDispatchGuard,
) -> Result<XReceipt> {
    dispatch(authorization, post, guard, None).await
}
pub(super) async fn dispatch_reserved_live(
    authorization: &AuthorizationReceipt,
    post: &ReservedPost<'_>,
    guard: &dyn MockDispatchGuard,
    live: &super::live::LiveAuthorization,
) -> Result<XReceipt> {
    dispatch(authorization, post, guard, Some(live)).await
}
async fn dispatch(
    authorization: &AuthorizationReceipt,
    post: &ReservedPost<'_>,
    guard: &dyn MockDispatchGuard,
    live: Option<&super::live::LiveAuthorization>,
) -> Result<XReceipt> {
    post.validate(now()?)?;
    let owner = "6fd863e1-745d-4a06-b68e-eea69e854a69"
        .parse()
        .map_err(|_| Error::Denied)?;
    if !authorization.matches(owner, NAMESPACE) {
        return Err(Error::Denied);
    }
    authorization.check()?;
    // Check durable authorization before even reading credentials or spending.
    authorize_reservation(post, guard).await?;
    authorization.check()?;
    let raw = PrivateKeyring::new(owner)?.read_oauth1_env()?;
    let credentials: Credentials = serde_json::from_slice(&raw).map_err(|_| Error::KeyVault)?;
    credentials.validate()?;
    let identity = IdentityClient::new()?;
    let wire = PostingTransport {
        client: identity.client,
        credentials,
        identity_url: identity.endpoint,
        post_url: POST_ENDPOINT.into(),
        session: Some(authorization),
        live_authorization: live,
        expires: post.expires_unix,
    };
    let adapter = MockXAdapter::prepared_live(
        wire,
        Zeroizing::new(String::new()),
        post.account.clone(),
        post.expires_unix,
    )?;
    Ok(adapter
        .execute_reserved_once(post.text, post.text_sha256, now()?, guard)
        .await)
}
/// Offline credential availability only, used before a durable reservation.
pub(crate) fn credentials_available(authorization: &AuthorizationReceipt) -> Result<()> {
    let owner = "6fd863e1-745d-4a06-b68e-eea69e854a69"
        .parse()
        .map_err(|_| Error::Denied)?;
    if !authorization.matches(owner, NAMESPACE) {
        return Err(Error::Denied);
    }
    authorization.check()?;
    let raw = PrivateKeyring::new(owner)?.read_oauth1_env()?;
    let credentials: Credentials = serde_json::from_slice(&raw).map_err(|_| Error::KeyVault)?;
    credentials.validate()
}
#[cfg(test)]
pub(crate) async fn test_posting(client: reqwest::Client, origin: &str, allowed: bool) -> XReceipt {
    assert!(origin.starts_with("https://127.0.0.1:"));
    struct Guard(bool);
    #[async_trait]
    impl MockDispatchGuard for Guard {
        async fn check_reserved(&self, _: &str, _: &MockAccount) -> Result<()> {
            if self.0 { Ok(()) } else { Err(Error::Denied) }
        }
        async fn authorize_exact(&self, _: &str, _: &MockAccount) -> Result<()> {
            if self.0 { Ok(()) } else { Err(Error::Denied) }
        }
    }
    let credentials:Credentials=serde_json::from_value(serde_json::json!({"X_API_KEY":"fixture-consumer-key","X_API_SECRET":"fixture-consumer-secret","X_ACCESS_TOKEN":"fixture-access-token","X_ACCESS_SECRET":"fixture-access-secret"})).unwrap();
    let wire = PostingTransport {
        client,
        credentials,
        identity_url: format!("{origin}/2/users/me"),
        post_url: format!("{origin}/2/tweets"),
        session: None,
        live_authorization: None,
        expires: now().unwrap() + 60,
    };
    let text = "Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip.";
    let hash = super::text_sha256(text);
    let post = ReservedPost {
        account: MockAccount {
            id: "123".into(),
            username: "supa_agency".into(),
            mode: "live".into(),
        },
        text,
        text_sha256: &hash,
        expires_unix: now().unwrap() + 60,
        budget_microusd: 25000,
    };
    if authorize_reservation(&post, &Guard(allowed)).await.is_err() {
        return XReceipt {
            mode: "live".into(),
            state: "failed".into(),
            category: "approval_revoked_or_unavailable".into(),
            post_id: None,
            http_status: None,
            rate_limit_reset: None,
        };
    }
    let adapter = MockXAdapter::prepared_live(
        wire,
        Zeroizing::new(String::new()),
        MockAccount {
            id: "123".into(),
            username: "supa_agency".into(),
            mode: "live".into(),
        },
        now().unwrap() + 60,
    )
    .unwrap();
    let result = adapter
        .execute_reserved_once(
            text,
            &super::text_sha256(text),
            now().unwrap(),
            &Guard(allowed),
        )
        .await;
    assert_eq!(
        adapter
            .execute_reserved_once(
                text,
                &super::text_sha256(text),
                now().unwrap(),
                &Guard(true)
            )
            .await
            .category,
        "attempt_already_consumed"
    );
    result
}
#[cfg(test)]
pub(crate) async fn test_identity(
    client: reqwest::Client,
    origin: &str,
) -> Result<IdentityReceipt> {
    assert!(origin.starts_with("https://127.0.0.1:"));
    let credentials = Credentials {
        api_key: Zeroizing::new("fixture-consumer-key".into()),
        api_secret: Zeroizing::new("fixture-consumer-secret".into()),
        access_token: Zeroizing::new("fixture-access-token".into()),
        access_secret: Zeroizing::new("fixture-access-secret".into()),
    };
    IdentityClient {
        client,
        endpoint: format!("{origin}/2/users/me"),
    }
    .once(&credentials)
    .await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reserved_posts_require_exact_identity_text_expiry_and_budget() {
        let at = now().unwrap();
        let text = "Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip.";
        let hash = super::super::text_sha256(text);
        let make = || ReservedPost {
            account: MockAccount {
                id: "123".into(),
                username: "supa_agency".into(),
                mode: "live".into(),
            },
            text,
            text_sha256: &hash,
            expires_unix: at + 60,
            budget_microusd: 25000,
        };
        make().validate(at).unwrap();
        for expiry in [at, at + 601] {
            let mut p = make();
            p.expires_unix = expiry;
            assert!(p.validate(at).is_err());
        }
        for budget in [0, 15000, 24999, 25001] {
            let mut p = make();
            p.budget_microusd = budget;
            assert!(p.validate(at).is_err());
        }
        for id in ["0", "01", "other", "18446744073709551616"] {
            let mut p = make();
            p.account.id = id.into();
            assert!(p.validate(at).is_err());
        }
        let mut p = make();
        p.account.username = "other".into();
        assert!(p.validate(at).is_err());
        let mut p = make();
        p.account.mode = "mock".into();
        assert!(p.validate(at).is_err());
        let mut p = make();
        p.text = "Changed after approval.";
        assert!(p.validate(at).is_err());
        let mut p = make();
        p.text_sha256 = "bad";
        assert!(p.validate(at).is_err());
    }
    #[test]
    fn signing_encoding_and_fixed_fixture_are_deterministic() {
        assert_eq!(encode("!*'() +/"), "%21%2A%27%28%29%20%2B%2F");
        let credentials:Credentials=serde_json::from_value(serde_json::json!({"X_API_KEY":"fixture-consumer-key","X_API_SECRET":"fixture-consumer-secret",
            "X_ACCESS_TOKEN":"fixture-access-token","X_ACCESS_SECRET":"fixture-access-secret"})).unwrap();
        let first = credentials
            .header("GET", IDENTITY_ENDPOINT, "fixture-nonce", 1234567890)
            .unwrap();
        assert_eq!(
            *first,
            *credentials
                .header("GET", IDENTITY_ENDPOINT, "fixture-nonce", 1234567890)
                .unwrap()
        );
        assert!(first.starts_with("OAuth "));
        assert!(!first.contains("fixture-consumer-secret"));
        assert!(!first.contains("fixture-access-secret"));
        assert_ne!(
            *first,
            *credentials
                .header("GET", IDENTITY_ENDPOINT, "other-nonce", 1234567890)
                .unwrap()
        );
    }
}

#[cfg(test)]
mod hosted_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[test]
    fn hosted_custody_rejects_missing_unsafe_and_malformed_files() {
        let dir = tempfile::tempdir_in(std::env::var_os("HOME").unwrap()).unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.path().join("oauth1.json");
        assert!(service_credentials(&path).is_err());
        let fixture = serde_json::json!({"X_API_KEY":"fixture-key","X_API_SECRET":"fixture-secret","X_ACCESS_TOKEN":"fixture-access","X_ACCESS_SECRET":"fixture-access-secret"});
        std::fs::write(&path, fixture.to_string()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(service_credentials(&path).is_ok());
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(service_credentials(&path).is_err());
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(service_credentials(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(service_credentials(&link).is_err());
        let hard = dir.path().join("hard");
        std::fs::hard_link(&path, &hard).unwrap();
        assert!(service_credentials(&path).is_err());
        std::fs::remove_file(hard).unwrap();
        for body in ["{}".to_owned(), "not-json".to_owned(), "x".repeat(16385)] {
            std::fs::write(&path, body).unwrap();
            assert!(service_credentials(&path).is_err());
        }
    }
}
