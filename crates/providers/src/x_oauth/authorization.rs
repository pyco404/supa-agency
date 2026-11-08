//! Fixed administrator-owned broker. No environment/path override or local receipt fallback.
use serde::Deserialize;
use std::{
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use supa_domain::{Error, Result};
use uuid::Uuid;

const HELPER: &str = "/usr/local/libexec/supa-x-owner-authorize";
const OWNER: &str = "6fd863e1-745d-4a06-b68e-eea69e854a69";

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Metadata {
    receipt_id: Uuid,
    owner_id: Uuid,
    client_id: String,
    expires_at: u64,
    proof_nonce: Uuid,
    consumed_nonce: bool,
    publishing_enabled: bool,
}

/// Non-cloneable consumed authority, private to this one connection attempt.
pub struct AuthorizationReceipt {
    metadata: Metadata,
}
impl AuthorizationReceipt {
    pub fn consume(receipt_id: Uuid, owner: Uuid, client: &str) -> Result<Self> {
        if receipt_id.is_nil()
            || owner.to_string() != OWNER
            || client.is_empty()
            || client.len() > 200
            || !client
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::Denied);
        }
        let metadata = invoke("consume", receipt_id, owner, client)?;
        validate(&metadata, receipt_id, owner, client)?;
        Ok(Self { metadata })
    }
    pub fn expires(&self) -> u64 {
        self.metadata.expires_at
    }
    pub fn matches(&self, owner: Uuid, client: &str) -> bool {
        self.metadata.owner_id == owner && self.metadata.client_id == client
    }
    pub fn check(&self) -> Result<()> {
        let current = invoke(
            "check",
            self.metadata.receipt_id,
            self.metadata.owner_id,
            &self.metadata.client_id,
        )?;
        validate(
            &current,
            self.metadata.receipt_id,
            self.metadata.owner_id,
            &self.metadata.client_id,
        )?;
        if current != self.metadata {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
fn validate(value: &Metadata, receipt: Uuid, owner: Uuid, client: &str) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Denied)?
        .as_secs();
    if value.receipt_id != receipt
        || value.owner_id != owner
        || value.client_id != client
        || value.proof_nonce.is_nil()
        || !value.consumed_nonce
        || value.publishing_enabled
        || value.expires_at <= now
        || value.expires_at > now.saturating_add(600)
    {
        return Err(Error::Denied);
    }
    Ok(())
}
fn trusted_helper() -> Result<()> {
    let path = Path::new(HELPER);
    for part in std::iter::once(path).chain(path.ancestors().skip(1)) {
        let metadata = fs::symlink_metadata(part).map_err(|_| Error::Denied)?;
        if metadata.file_type().is_symlink()
            || metadata.uid() != 0
            || metadata.mode() & 0o022 != 0
            || (part == path && (!metadata.is_file() || metadata.nlink() != 1))
        {
            return Err(Error::Denied);
        }
    }
    Ok(())
}
fn invoke(operation: &str, receipt: Uuid, owner: Uuid, client: &str) -> Result<Metadata> {
    trusted_helper()?;
    let mut child = Command::new("/usr/bin/sudo")
        .args([
            "-n",
            "--",
            HELPER,
            operation,
            &receipt.to_string(),
            &owner.to_string(),
            client,
        ])
        .env_clear()
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::Denied)?;
    let result = (|| {
        let began = Instant::now();
        loop {
            match child.try_wait().map_err(|_| Error::Denied)? {
                Some(status) => {
                    if !status.success() {
                        return Err(Error::Denied);
                    }
                    break;
                }
                None if began.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                None => return Err(Error::Timeout),
            }
        }
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or(Error::Denied)?
            .take(2049)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Denied)?;
        if bytes.len() > 2048 {
            return Err(Error::Denied);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Denied)
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_metadata_cannot_substitute_owner_client_nonce_or_expiry() {
        let receipt = Uuid::from_u128(3);
        let owner = Uuid::parse_str(OWNER).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut data = serde_json::json!({"receipt_id":receipt,"owner_id":owner,"client_id":"fixture_client",
            "expires_at":now+60,"proof_nonce":Uuid::from_u128(4),"consumed_nonce":true,"publishing_enabled":false});
        let parsed: Metadata = serde_json::from_value(data.clone()).unwrap();
        assert!(validate(&parsed, receipt, owner, "fixture_client").is_ok());
        for (field, value) in [
            ("owner_id", serde_json::json!(Uuid::from_u128(99))),
            ("client_id", serde_json::json!("different")),
            ("proof_nonce", serde_json::json!(Uuid::nil())),
            ("consumed_nonce", serde_json::json!(false)),
            ("publishing_enabled", serde_json::json!(true)),
            ("expires_at", serde_json::json!(now)),
            ("expires_at", serde_json::json!(now + 601)),
        ] {
            let mut bad = data.clone();
            bad[field] = value;
            let parsed: Metadata = serde_json::from_value(bad).unwrap();
            assert!(validate(&parsed, receipt, owner, "fixture_client").is_err());
        }
        data["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Metadata>(data).is_err());
        assert!(AuthorizationReceipt::consume(Uuid::nil(), owner, "fixture_client").is_err());
        assert!(
            AuthorizationReceipt::consume(receipt, Uuid::from_u128(9), "fixture_client").is_err()
        );
    }
}
