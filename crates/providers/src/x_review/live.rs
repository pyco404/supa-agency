//! Prepared live boundary. No environment activation, token export, refresh or worker.
//! The root-installed non-secret receipt is distinct from OAuth connection consent.
use super::*;
use crate::x_oauth::local::PrivateKeyring;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const ROOT: &str = "/etc/supa-agent/x-autonomous-publishing";
const OWNER: &str = "6fd863e1-745d-4a06-b68e-eea69e854a69";
pub(crate) fn numeric_id(s: &str) -> bool {
    !s.starts_with('0')
        && s.parse::<u64>().is_ok_and(|n| n > 0)
        && s.bytes().all(|b| b.is_ascii_digit())
}
fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::Denied)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u8,
    receipt_id: Uuid,
    owner_id: Uuid,
    local_uid: u32,
    client_id: String,
    account: MockAccount,
    callback: String,
    custody: String,
    scope: String,
    owner_proof_sha256: String,
    owner_verification_approved: bool,
    autonomous_activation_approved: bool,
    issued_unix: u64,
    expires_unix: u64,
    grant: Value,
    identity_read_microusd: u64,
    post_create_microusd: u64,
}
impl Receipt {
    fn validate(&self, id: Uuid, at: u64) -> Result<()> {
        if self.version != 1
            || self.receipt_id != id
            || id.is_nil()
            || self.owner_id.to_string() != OWNER
            || self.local_uid == 0
            || std::fs::metadata("/proc/self")
                .map_err(|_| Error::Denied)?
                .uid()
                != self.local_uid
            || !numeric_id(&self.account.id)
            || self.account.mode != "live"
            || self.account.username != "supa_agency"
            || self.callback != crate::x_oauth::REGISTERED_CALLBACK
            || self.custody != crate::x_oauth::CREDENTIAL_CUSTODY
            || self.scope != "original_text_released_metadata_only"
            || !self.owner_verification_approved
            || !self.autonomous_activation_approved
            || self.client_id.is_empty()
            || self.client_id.len() > 200
            || !self
                .client_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || self.owner_proof_sha256.len() != 64
            || !self
                .owner_proof_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || self.issued_unix > at
            || self.expires_unix <= at
            || self.expires_unix <= self.issued_unix
            || self.expires_unix - self.issued_unix > 604800
            || self.identity_read_microusd != 10000
            || self.post_create_microusd != 15000
        {
            return Err(Error::Denied);
        }
        let g = &self.grant;
        let keys = [
            "grant_id",
            "account",
            "confirmed",
            "scope",
            "expires_at",
            "max_posts",
            "interval_seconds",
            "budget_microusd",
            "activation_receipt_id",
            "owner_proof_sha256",
        ];
        let valid_keys = g
            .as_object()
            .is_some_and(|m| m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k)));
        if !valid_keys
            || g["grant_id"]
                .as_str()
                .and_then(|s| s.parse::<Uuid>().ok())
                .is_none_or(|u| u.is_nil())
            || g["account"] != serde_json::to_value(&self.account).map_err(|_| Error::Denied)?
            || g["activation_receipt_id"] != self.receipt_id.to_string()
            || g["owner_proof_sha256"] != self.owner_proof_sha256
            || g["confirmed"] != true
            || g["scope"] != self.scope
            || !g["max_posts"]
                .as_u64()
                .is_some_and(|n| (1..=6).contains(&n))
            || !g["interval_seconds"]
                .as_u64()
                .is_some_and(|n| (14400..=86400).contains(&n))
            || !g["budget_microusd"]
                .as_u64()
                .is_some_and(|n| (25000..=10000000).contains(&n))
            || g["expires_at"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .is_none_or(|v| v.timestamp() < 0 || v.timestamp() as u64 != self.expires_unix)
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
/// Deliberately non-cloneable. Root-controlled approval and revocation checked
/// before custody access, identity lookup and dispatch; edits invalidate the hash.
pub struct LiveAuthorization {
    receipt: Receipt,
    digest: String,
}
fn protected(path: &Path, directory: bool) -> Result<()> {
    let m = std::fs::symlink_metadata(path).map_err(|_| Error::Denied)?;
    if m.uid() != 0
        || m.permissions().mode() & 0o022 != 0
        || m.file_type().is_symlink()
        || (directory && !m.is_dir())
        || (!directory && (!m.is_file() || m.nlink() != 1 || m.len() > 16384))
    {
        return Err(Error::Denied);
    }
    Ok(())
}
fn read_receipt(id: Uuid) -> Result<Vec<u8>> {
    if id.is_nil() {
        return Err(Error::Denied);
    }
    let root = Path::new(ROOT);
    for p in root.ancestors() {
        protected(p, true)?;
    }
    for p in [root.join("live"), root.join("revoked")] {
        protected(&p, true)?;
    }
    // Any marker or filesystem error fails closed. No file is created here.
    match std::fs::symlink_metadata(root.join("revoked").join(format!("{id}.json"))) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(Error::Denied),
    }
    let file: PathBuf = root.join("live").join(format!("{id}.json"));
    protected(&file, false)?;
    std::fs::read(file).map_err(|_| Error::Denied)
}
impl LiveAuthorization {
    pub fn load(id: Uuid) -> Result<Self> {
        let raw = read_receipt(id)?;
        let receipt: Receipt = serde_json::from_slice(&raw).map_err(|_| Error::Denied)?;
        receipt.validate(id, now()?)?;
        Ok(Self {
            receipt,
            digest: text_sha256(std::str::from_utf8(&raw).map_err(|_| Error::Denied)?),
        })
    }
    pub fn check(&self) -> Result<()> {
        self.receipt.validate(self.receipt.receipt_id, now()?)?;
        let raw = read_receipt(self.receipt.receipt_id)?;
        if text_sha256(std::str::from_utf8(&raw).map_err(|_| Error::Denied)?) != self.digest {
            return Err(Error::Denied);
        }
        Ok(())
    }
    pub fn owner(&self) -> Uuid {
        self.receipt.owner_id
    }
    pub fn grant(&self) -> &Value {
        &self.receipt.grant
    }
    pub fn account(&self) -> &MockAccount {
        &self.receipt.account
    }
    pub fn is_hosted_oauth1(&self) -> bool {
        self.receipt.client_id == super::oauth1::HOSTED_NAMESPACE
    }
    pub fn expires_unix(&self) -> u64 {
        self.receipt.expires_unix
    }
    pub(crate) fn client_id(&self) -> &str {
        &self.receipt.client_id
    }
}
/// Server-only custody payload. No Debug/Clone/Serialize or public token accessor.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Connection {
    pub(crate) owner: Uuid,
    pub(crate) client_id: String,
    pub(crate) account_id: String,
    pub(crate) username: String,
    pub(crate) scope: Vec<String>,
    pub(crate) expires_unix: u64,
    #[serde(deserialize_with = "private_token")]
    pub(crate) access_token: Zeroizing<String>,
}
fn private_token<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Zeroizing<String>, D::Error> {
    <String as serde::Deserialize>::deserialize(d).map(Zeroizing::new)
}
impl Connection {
    pub(crate) fn validate(&self, auth: &LiveAuthorization) -> Result<()> {
        if self.owner != auth.owner()
            || self.client_id != auth.client_id()
            || self.account_id != auth.account().id
            || self.username != "supa_agency"
            || self.scope != SCOPES
            || self.expires_unix <= now()?
            || self.access_token.is_empty()
            || self.access_token.len() > 4096
            || !self.access_token.bytes().all(|b| (33..=126).contains(&b))
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
/// Fixed HTTPS destinations; no redirects, proxy, endpoint overrides or retries.
struct HttpsPosting {
    client: reqwest::Client,
    identity_url: String,
    post_url: String,
}
impl HttpsPosting {
    fn new() -> Result<Self> {
        Ok(Self {
            identity_url: IDENTITY_ENDPOINT.into(),
            post_url: POST_ENDPOINT.into(),
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
}
#[async_trait]
impl MockXTransport for HttpsPosting {
    async fn exchange(
        &self,
        path: &str,
        body: Option<Value>,
        credential: &str,
    ) -> Result<MockResponse> {
        let mut header = reqwest::header::HeaderValue::from_str(&format!("Bearer {credential}"))
            .map_err(|_| Error::Denied)?;
        header.set_sensitive(true);
        let request = match (path, body) {
            ("/2/users/me", None) => self.client.get(&self.identity_url),
            ("/2/tweets", Some(body))
                if body
                    .as_object()
                    .is_some_and(|m| m.len() == 1 && m.contains_key("text")) =>
            {
                safe_post(body["text"].as_str().ok_or(Error::Denied)?)?;
                self.client.post(&self.post_url).json(&body)
            }
            _ => return Err(Error::Denied),
        };
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
        // Known rejections need no provider body. Never retain raw errors.
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
struct AuthorizedTransport<'a> {
    wire: HttpsPosting,
    auth: &'a LiveAuthorization,
    token_expires: u64,
}
#[async_trait]
impl MockXTransport for AuthorizedTransport<'_> {
    async fn exchange(
        &self,
        path: &str,
        body: Option<Value>,
        credential: &str,
    ) -> Result<MockResponse> {
        self.auth.check()?;
        if now()? >= self.token_expires {
            return Err(Error::Denied);
        }
        self.wire.exchange(path, body, credential).await
    }
}
/// Metadata-only result; the token stays in private custody/zeroizing backend memory.
/// Does not perform a paid identity read or refresh.
pub fn verify_connection(auth: &LiveAuthorization) -> Result<u64> {
    auth.check()?;
    if auth.client_id() == super::oauth1::HOSTED_NAMESPACE {
        super::oauth1::hosted_credentials_available(auth)?;
        return Ok(auth.expires_unix());
    }
    if auth.client_id() == super::oauth1::NAMESPACE {
        let session =
            crate::x_oauth::local::authorization::AuthorizationReceipt::local_owner_session(
                super::oauth1::NAMESPACE,
            )?;
        super::oauth1::credentials_available(&session)?;
        auth.check()?;
        return Ok(auth.expires_unix());
    }
    let connection = PrivateKeyring::new(auth.owner())?.posting_connection(auth)?;
    connection.validate(auth)?;
    auth.check()?;
    Ok(connection.expires_unix.min(auth.expires_unix()))
}
/// One pre-reserved attempt only. Caller supplies a durable, ownership-checked
/// dispatch guard. No connection receipt or terminal confirmation can activate it.
pub async fn dispatch_reserved_once(
    auth: &LiveAuthorization,
    text: &str,
    hash: &str,
    guard: &dyn MockDispatchGuard,
) -> Result<XReceipt> {
    auth.check()?;
    if auth.client_id() == super::oauth1::HOSTED_NAMESPACE {
        return super::oauth1::dispatch_hosted(auth, text, hash, guard).await;
    }
    if auth.client_id() == super::oauth1::NAMESPACE {
        let session =
            crate::x_oauth::local::authorization::AuthorizationReceipt::local_owner_session(
                super::oauth1::NAMESPACE,
            )?;
        // Keep the root authorization/revocation check on BOTH transport stages.
        struct Guard<'a> {
            auth: &'a LiveAuthorization,
            durable: &'a dyn MockDispatchGuard,
        }
        #[async_trait]
        impl MockDispatchGuard for Guard<'_> {
            async fn check_reserved(&self, hash: &str, account: &MockAccount) -> Result<()> {
                self.auth.check()?;
                self.durable.check_reserved(hash, account).await
            }
            async fn authorize_exact(&self, hash: &str, account: &MockAccount) -> Result<()> {
                self.auth.check()?;
                self.durable.authorize_exact(hash, account).await
            }
        }
        let guarded = Guard {
            auth,
            durable: guard,
        };
        return super::oauth1::dispatch_reserved_live(
            &session,
            &super::oauth1::ReservedPost {
                account: auth.account().clone(),
                text,
                text_sha256: hash,
                expires_unix: auth.expires_unix().min(now()?.saturating_add(600)),
                budget_microusd: 25000,
            },
            &guarded,
            auth,
        )
        .await;
    }
    let connection = PrivateKeyring::new(auth.owner())?.posting_connection(auth)?;
    connection.validate(auth)?;
    let expires = connection.expires_unix.min(auth.expires_unix());
    let wire = AuthorizedTransport {
        wire: HttpsPosting::new()?,
        auth,
        token_expires: expires,
    };
    let adapter = MockXAdapter::prepared_live(
        wire,
        connection.access_token,
        auth.account().clone(),
        expires,
    )?;
    Ok(adapter
        .execute_reserved_once(text, hash, now()?, guard)
        .await)
}

#[cfg(test)]
pub(crate) async fn test_https_wire(client: reqwest::Client, origin: &str) -> XReceipt {
    assert!(origin.starts_with("https://127.0.0.1:"));
    let transport = HttpsPosting {
        client,
        identity_url: format!("{origin}/2/users/me"),
        post_url: format!("{origin}/2/tweets"),
    };
    struct TestGuard;
    #[async_trait]
    impl MockDispatchGuard for TestGuard {
        async fn authorize_exact(&self, _: &str, _: &MockAccount) -> Result<()> {
            Ok(())
        }
    }
    let adapter = MockXAdapter::prepared_live(
        transport,
        Zeroizing::new("fixture-access-token-only".into()),
        MockAccount {
            id: "123".into(),
            username: "supa_agency".into(),
            mode: "live".into(),
        },
        now().unwrap() + 30,
    )
    .unwrap();
    let text = "A genuine synthetic fixture result.";
    adapter
        .execute_reserved_once(text, &text_sha256(text), now().unwrap(), &TestGuard)
        .await
}

#[cfg(test)]
mod tests;
