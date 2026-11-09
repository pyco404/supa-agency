//! OAuth 1.0a identity-only adapter. No POST, consent flow, refresh or publishing.
use super::{IDENTITY_ENDPOINT, live::numeric_id};
use crate::x_oauth::local::{PrivateKeyring, authorization::AuthorizationReceipt};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use supa_domain::{Error, Result};
use zeroize::Zeroizing;

pub const NAMESPACE: &str = "oauth1-supa-agency";
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
    fn header(&self, url: &str, nonce: &str, at: u64) -> Result<Zeroizing<String>> {
        self.validate()?;
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
        let base = Zeroizing::new(format!("GET&{}&{}", encode(url), encode(&normalized)));
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
        let auth = credentials.header(&self.endpoint, &nonce, now()?)?;
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
    fn signing_encoding_and_fixed_fixture_are_deterministic() {
        assert_eq!(encode("!*'() +/"), "%21%2A%27%28%29%20%2B%2F");
        let credentials:Credentials=serde_json::from_value(serde_json::json!({"X_API_KEY":"fixture-consumer-key","X_API_SECRET":"fixture-consumer-secret",
            "X_ACCESS_TOKEN":"fixture-access-token","X_ACCESS_SECRET":"fixture-access-secret"})).unwrap();
        let first = credentials
            .header(IDENTITY_ENDPOINT, "fixture-nonce", 1234567890)
            .unwrap();
        assert_eq!(
            *first,
            *credentials
                .header(IDENTITY_ENDPOINT, "fixture-nonce", 1234567890)
                .unwrap()
        );
        assert!(first.starts_with("OAuth "));
        assert!(!first.contains("fixture-consumer-secret"));
        assert!(!first.contains("fixture-access-secret"));
        assert_ne!(
            *first,
            *credentials
                .header(IDENTITY_ENDPOINT, "other-nonce", 1234567890)
                .unwrap()
        );
    }
}
