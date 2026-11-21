//! Root-provisioned exact-text pilot permit. No creation, enrollment or scheduling.
use super::MockAccount;
#[cfg(not(feature = "pilot-offline-tests"))]
use super::oauth1;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use supa_domain::{Error, Result};
use uuid::Uuid;
const ROOT: &str = "/etc/supa-agent/x-pilot";
const OWNER: &str = "6fd863e1-745d-4a06-b68e-eea69e854a69";
#[cfg(feature = "pilot-offline-tests")]
#[path = "pilot_offline.rs"]
mod offline;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u8,
    pub authorization_id: Uuid,
    pub task_id: Uuid,
    pub owner_id: Uuid,
    pub account: MockAccount,
    pub text_sha256: String,
    pub identity_receipt_sha256: String,
    pub context_sha256: String,
    pub issued_unix: u64,
    pub expires_unix: u64,
    pub max_identity_gets: u8,
    pub max_posts: u8,
    pub budget_microusd: u64,
    pub approved: bool,
    pub read_write_confirmed: bool,
    pub credits_confirmed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    version: u8,
    image_digest: String,
    binary_sha256: String,
    ledger_cluster_id: String,
    ledger_import_sha256: String,
    runtime_uid: u32,
}
fn sha(raw: &[u8]) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, raw);
    d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn at() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| Error::Denied)
}
fn protected(path: &Path, directory: bool) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|_| Error::Denied)?;
    if m.uid() != 0
        || m.mode() & 0o022 != 0
        || m.file_type().is_symlink()
        || (directory && !m.is_dir())
        || (!directory && (!m.is_file() || m.nlink() != 1 || m.len() > 16384))
    {
        return Err(Error::Denied);
    }
    Ok(())
}
fn read(path: &Path) -> Result<Vec<u8>> {
    for p in path.ancestors().skip(1) {
        protected(p, true)?;
    }
    protected(path, false)?;
    let before = fs::symlink_metadata(path).map_err(|_| Error::Denied)?;
    let mut f = fs::OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|_| Error::Denied)?;
    let m = f.metadata().map_err(|_| Error::Denied)?;
    if m.ino() != before.ino() || m.dev() != before.dev() {
        return Err(Error::Denied);
    }
    let mut b = Vec::new();
    f.by_ref()
        .take(16385)
        .read_to_end(&mut b)
        .map_err(|_| Error::Denied)?;
    if b.len() > 16384 {
        return Err(Error::Denied);
    }
    Ok(b)
}
fn validate(
    raw: &[u8],
    context: &[u8],
    id: Uuid,
    now: u64,
    uid: u32,
    binary: &str,
) -> Result<Receipt> {
    let r: Receipt = serde_json::from_slice(raw).map_err(|_| Error::Denied)?;
    let c: Context = serde_json::from_slice(context).map_err(|_| Error::Denied)?;
    if r.version != 1
        || r.authorization_id != id
        || id.is_nil()
        || r.task_id.is_nil()
        || r.owner_id.to_string() != OWNER
        || r.account.mode != "live"
        || r.account.username != "supa_agency"
        || !super::live::numeric_id(&r.account.id)
        || !r.approved
        || !r.read_write_confirmed
        || !r.credits_confirmed
        || !hash(&r.text_sha256)
        || !hash(&r.identity_receipt_sha256)
        || r.context_sha256 != sha(context)
        || r.issued_unix > now
        || r.expires_unix <= now
        || r.expires_unix <= r.issued_unix
        || r.expires_unix - r.issued_unix > 600
        || r.max_identity_gets != 1
        || r.max_posts != 1
        || r.budget_microusd != 25000
        || c.version != 1
        || uid != 10001
        || c.runtime_uid != uid
        || c.binary_sha256 != binary
        || !hash(binary)
        || !c.image_digest.strip_prefix("sha256:").is_some_and(hash)
        || !hash(&c.ledger_import_sha256)
        || c.ledger_cluster_id.is_empty()
        || !c.ledger_cluster_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(Error::Denied);
    }
    Ok(r)
}
/// Only load() can construct the permit; callers cannot mint one from environment.
pub struct PilotAuthorization {
    receipt: Receipt,
    digest: String,
    context: Vec<u8>,
    binary: String,
    uid: u32,
    path: PathBuf,
}
impl PilotAuthorization {
    pub fn load(id: Uuid) -> Result<Self> {
        let uid = fs::metadata("/proc/self").map_err(|_| Error::Denied)?.uid();
        let root = Path::new(ROOT);
        protected(root, true)?;
        protected(&root.join("revoked"), true)?;
        if !absent(&root.join("stop")) || !absent(&root.join("revoked").join(id.to_string())) {
            return Err(Error::Denied);
        }
        let context = read(&root.join("context.json"))?;
        let path = root.join("approved").join(format!("{id}.json"));
        let raw = read(&path)?;
        let binary = sha(&fs::read("/proc/self/exe").map_err(|_| Error::Denied)?);
        let receipt = validate(&raw, &context, id, at()?, uid, &binary)?;
        Ok(Self {
            receipt,
            digest: sha(&raw),
            context,
            binary,
            uid,
            path,
        })
    }
    pub fn check(&self) -> Result<()> {
        let root = Path::new(ROOT);
        protected(root, true)?;
        protected(&root.join("revoked"), true)?;
        if !absent(&root.join("stop"))
            || !absent(
                &root
                    .join("revoked")
                    .join(self.receipt.authorization_id.to_string()),
            )
        {
            return Err(Error::Denied);
        }
        let raw = read(&self.path)?;
        if sha(&raw) != self.digest || read(&root.join("context.json"))? != self.context {
            return Err(Error::Denied);
        }
        validate(
            &raw,
            &self.context,
            self.receipt.authorization_id,
            at()?,
            self.uid,
            &self.binary,
        )?;
        Ok(())
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn ledger_cluster_id(&self) -> Result<String> {
        Ok(serde_json::from_slice::<Context>(&self.context)
            .map_err(|_| Error::Denied)?
            .ledger_cluster_id)
    }
    pub fn approval(&self, text: &str) -> Result<Value> {
        self.check()?;
        supa_domain::social_review::safe_post(text)?;
        if super::text_sha256(text) != self.receipt.text_sha256 {
            return Err(Error::Denied);
        }
        Ok(
            serde_json::json!({"authorization_id":self.receipt.authorization_id,"task_id":self.receipt.task_id,"receipt_sha256":self.digest,
   "account":self.receipt.account,"text":text,"text_sha256":self.receipt.text_sha256,
   "expires_at":chrono::DateTime::from_timestamp(self.receipt.expires_unix as i64,0).ok_or(Error::Denied)?.to_rfc3339(),
   "budget_microusd":25000,"confirmed":true,"read_write_confirmed":true,"credits_confirmed":true,"identity_receipt_sha256":self.receipt.identity_receipt_sha256}),
        )
    }
    pub async fn dispatch(
        &self,
        text: &str,
        guard: &dyn super::MockDispatchGuard,
    ) -> Result<super::XReceipt> {
        #[cfg(not(feature = "pilot-offline-tests"))]
        {
            oauth1::dispatch_pilot(self, text, guard).await
        }
        #[cfg(feature = "pilot-offline-tests")]
        {
            offline::dispatch(self, text, guard).await
        }
    }
}

fn absent(path: &Path) -> bool {
    matches!(fs::symlink_metadata(path), Err(error) if error.kind()==std::io::ErrorKind::NotFound)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::x_review::{MockDispatchGuard, MockResponse, MockXAdapter, MockXTransport};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    fn fixture() -> (Value, Vec<u8>, Uuid, String) {
        let binary = "a".repeat(64);
        let id = Uuid::from_u128(1);
        let context=serde_json::to_vec(&json!({"version":1,"image_digest":format!("sha256:{}","b".repeat(64)),"binary_sha256":binary,"ledger_cluster_id":"123","ledger_import_sha256":"c".repeat(64),"runtime_uid":10001})).unwrap();
        let value = json!({"version":1,"authorization_id":id,"task_id":Uuid::from_u128(2),"owner_id":OWNER,"account":{"id":"123","username":"supa_agency","mode":"live"},"text_sha256":"d".repeat(64),"identity_receipt_sha256":"e".repeat(64),"context_sha256":sha(&context),"issued_unix":100,"expires_unix":700,"max_identity_gets":1,"max_posts":1,"budget_microusd":25000,"approved":true,"read_write_confirmed":true,"credits_confirmed":true});
        (value, context, id, binary)
    }
    fn valid(v: &Value, c: &[u8], id: Uuid, b: &str) -> bool {
        validate(&serde_json::to_vec(v).unwrap(), c, id, 101, 10001, b).is_ok()
    }
    #[test]
    fn exact_protected_contract() {
        let (v, c, id, b) = fixture();
        assert!(valid(&v, &c, id, &b));
    }
    #[test]
    fn missing_expired_or_unapproved_denies() {
        let (v, c, id, b) = fixture();
        assert!(!valid(&json!({}), &c, id, &b));
        for (k, x) in [
            ("approved", json!(false)),
            ("expires_unix", json!(101)),
            ("issued_unix", json!(102)),
            ("expires_unix", json!(701)),
        ] {
            let mut w = v.clone();
            w[k] = x;
            assert!(!valid(&w, &c, id, &b));
        }
    }
    #[test]
    fn count_budget_owner_and_account_denies() {
        let (v, c, id, b) = fixture();
        for (k, x) in [
            ("max_posts", json!(2)),
            ("max_identity_gets", json!(2)),
            ("budget_microusd", json!(25001)),
            ("owner_id", json!(Uuid::from_u128(3))),
            ("task_id", json!(Uuid::nil())),
        ] {
            let mut w = v.clone();
            w[k] = x;
            assert!(!valid(&w, &c, id, &b));
        }
        let mut w = v;
        w["account"]["username"] = json!("other");
        assert!(!valid(&w, &c, id, &b));
    }
    #[test]
    fn context_hash_binary_uid_and_unknown_fields_deny() {
        let (v, c, id, b) = fixture();
        let mut w = v.clone();
        w["context_sha256"] = json!("0".repeat(64));
        assert!(!valid(&w, &c, id, &b));
        w = v.clone();
        w["text_sha256"] = json!("not-a-hash");
        assert!(!valid(&w, &c, id, &b));
        w = v.clone();
        w["autonomous_activation_approved"] = json!(true);
        assert!(!valid(&w, &c, id, &b));
        assert!(!valid(&v, &c, id, &"f".repeat(64)));
        assert!(validate(&serde_json::to_vec(&v).unwrap(), &c, id, 101, 0, &b).is_err());
    }
    #[test]
    fn emergency_stop_including_dangling_marker() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("stop");
        assert!(absent(&p));
        std::os::unix::fs::symlink(t.path().join("missing"), &p).unwrap();
        assert!(!absent(&p));
        assert!(PilotAuthorization::load(Uuid::nil()).is_err());
    }
    struct Guard {
        claimed: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
    }
    #[async_trait]
    impl MockDispatchGuard for Guard {
        async fn check_reserved(&self, _: &str, _: &MockAccount) -> Result<()> {
            if self.stop.load(Ordering::SeqCst) || self.claimed.load(Ordering::SeqCst) {
                Err(Error::Denied)
            } else {
                Ok(())
            }
        }
        async fn authorize_exact(&self, _: &str, _: &MockAccount) -> Result<()> {
            self.check_reserved(
                "",
                &MockAccount {
                    id: "123".into(),
                    username: "supa_agency".into(),
                    mode: "live".into(),
                },
            )
            .await?;
            if self.claimed.swap(true, Ordering::SeqCst) {
                Err(Error::Denied)
            } else {
                Ok(())
            }
        }
    }
    struct Wire {
        calls: Arc<AtomicUsize>,
        ambiguous: bool,
        wrong_account: bool,
    }
    #[async_trait]
    impl MockXTransport for Wire {
        async fn exchange(&self, path: &str, body: Option<Value>, _: &str) -> Result<MockResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if path == "/2/users/me" {
                return Ok(MockResponse{status:200,body:serde_json::to_vec(&json!({"data":{"id":if self.wrong_account{"456"}else{"123"},"username":"supa_agency"}})).unwrap(),rate_limit_reset:None});
            }
            if self.ambiguous {
                return Err(Error::ProviderTransport);
            }
            Ok(MockResponse {
                status: 201,
                body: serde_json::to_vec(
                    &json!({"data":{"id":"789","text":body.unwrap()["text"]}}),
                )
                .unwrap(),
                rate_limit_reset: None,
            })
        }
    }
    async fn scenario(ambiguous: bool, wrong: bool, stopped: bool) -> (String, usize, bool) {
        let calls = Arc::new(AtomicUsize::new(0));
        let claimed = Arc::new(AtomicBool::new(false));
        let g = Guard {
            claimed: claimed.clone(),
            stop: Arc::new(AtomicBool::new(stopped)),
        };
        let a = MockAccount {
            id: "123".into(),
            username: "supa_agency".into(),
            mode: "live".into(),
        };
        if g.check_reserved("", &a).await.is_err() {
            return ("denied".into(), calls.load(Ordering::SeqCst), false);
        }
        let adapter = MockXAdapter::prepared_live(
            Wire {
                calls: calls.clone(),
                ambiguous,
                wrong_account: wrong,
            },
            zeroize::Zeroizing::new(String::new()),
            a.clone(),
            at().unwrap() + 60,
        )
        .unwrap();
        let text = "Synthetic pilot fixture only.";
        let hash = super::super::text_sha256(text);
        let r = adapter
            .execute_reserved_once(text, &hash, at().unwrap(), &g)
            .await;
        let count = calls.load(Ordering::SeqCst);
        let again = adapter
            .execute_reserved_once(text, &hash, at().unwrap(), &g)
            .await;
        assert_eq!(again.state, "failed");
        assert_eq!(calls.load(Ordering::SeqCst), count);
        (r.state, count, claimed.load(Ordering::SeqCst))
    }
    #[tokio::test]
    async fn successful_synthetic_one_shot() {
        assert_eq!(
            scenario(false, false, false).await,
            ("posted".into(), 2, true)
        );
    }
    #[tokio::test]
    async fn uncertain_outcome_never_retries() {
        assert_eq!(
            scenario(true, false, false).await,
            ("uncertain".into(), 2, true)
        );
    }
    #[tokio::test]
    async fn wrong_identity_prevents_post() {
        assert_eq!(
            scenario(false, true, false).await,
            ("failed".into(), 1, false)
        );
    }
    #[tokio::test]
    async fn emergency_stop_before_identity() {
        assert_eq!(
            scenario(false, false, true).await,
            ("denied".into(), 0, false)
        );
    }
}
