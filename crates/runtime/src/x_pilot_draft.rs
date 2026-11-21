//! Shared exact-byte draft gate; never grants posting authority.
use serde_json::Value;
use supa_domain::{Error, Result};
use supa_providers::x_review::{self, MockAccount};
pub const TEXT: &str = "Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip.";
pub const HASH: &str = "e8bc2bee94696da203aad809d8a80aeb05a4677960ef34d9b0b68b82d47f2dd7";
// Draft preparation is offline. A matching hash is content identification, not
// owner authorization, a spending grant, or permission to use hosted credentials.
pub struct ExactDraft {
    pub text: String,
    pub text_sha256: String,
    pub account: MockAccount,
}
pub fn validate_draft(
    raw: &[u8],
    expected_hash: &str,
    account: &MockAccount,
) -> Result<ExactDraft> {
    let value: Value = serde_json::from_slice(raw).map_err(|_| Error::Denied)?;
    let fields = value.as_object().ok_or(Error::Denied)?;
    if fields.len() != 4
        || !["version", "text", "text_sha256", "account"]
            .iter()
            .all(|key| fields.contains_key(*key))
        || value["version"].as_u64() != Some(1)
    {
        return Err(Error::Denied);
    }
    let draft = ExactDraft {
        text: value["text"].as_str().ok_or(Error::Denied)?.to_owned(),
        text_sha256: value["text_sha256"]
            .as_str()
            .ok_or(Error::Denied)?
            .to_owned(),
        account: serde_json::from_value(value["account"].clone()).map_err(|_| Error::Denied)?,
    };
    let normalize = |text: &str| {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    if draft.account.id != account.id
        || draft.account.username != account.username
        || draft.account.mode != account.mode
        || draft.account.mode != "live"
        || draft.text.trim() != draft.text
        || expected_hash.len() != 64
        || !expected_hash
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || draft.text_sha256 != expected_hash
        || x_review::text_sha256(&draft.text) != expected_hash
        || expected_hash == HASH
        || normalize(&draft.text) == normalize(TEXT)
    {
        return Err(Error::Denied);
    }
    supa_domain::social_review::safe_post(&draft.text)?;
    Ok(draft)
}
