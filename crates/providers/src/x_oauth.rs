//! Offline OAuth preparation only. No listener, token exchange, credential loader
//! or HTTP client is installed. Mock tokens cannot authorize production posting.
use crate::x_review::SCOPES;
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
/// Non-secret public-client configuration. Registration is an owner action;
/// accepting a URL here does not establish that X has registered/accepted it.
pub struct OAuthPreparation {
    client_id: String,
    callback: Url,
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
        })
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
    }
    fn consume(&mut self, owner: Uuid, callback: &str, now: u64) -> Result<Zeroizing<String>> {
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
                ("client_id", &self.config.client_id),
                ("redirect_uri", self.config.callback.as_str()),
                ("code", code),
                ("code_verifier", &self.verifier),
            ]);
            Ok(Zeroizing::new(
                form.query().ok_or(Error::Invalid)?.to_owned(),
            ))
        })();
        self.verifier.zeroize();
        parsed
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
        let form = self.consume(owner, callback, now)?;
        let parsed = Url::parse(&format!("https://invalid.example/?{}", &*form))
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
}
