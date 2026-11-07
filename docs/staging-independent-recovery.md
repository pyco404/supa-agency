# Independent PostgreSQL 18 recovery preparation

Preparation and synthetic testing only. The actual Railway `supa_staging`
database is **not yet independently recovery-verified**. No live export,
production key generation/enrollment, bucket provisioning, credential change,
backup transfer, service deployment or database modification is authorized by
this document. The protected primary and historical accounting are excluded.

## Reuse and the smallest independent destination

Reuse `scripts/backup-crypto.py` for bounded authenticated encryption and signed
ciphertext verification, `scripts/test-backup-crypto.py` for crypto regressions,
and `persistent-postgres.py`'s full user-schema/data/constraints/index/sequence
fingerprint algorithm. Do **not** execute the latter's production-path actions:
its PG16 local source and primary paths are unrelated to this Railway backup.
`independent-backup-preflight.py` can verify local physical disks; it cannot
certify cloud storage or treat `/tmp`, RAM or another partition as independent.
Existing Railway volume/native backups remain supplementary, unchanged.

Recommend **one private Backblaze B2 bucket in an owner-controlled account**,
outside the Railway project/volume and GitHub, with owner MFA and account
recovery kept separately. It avoids adding a database or always-on backup VM.
Do not reuse the old Railway bucket or the protected-primary backup inventory.
The owner must confirm jurisdiction/region and independent account access before
provisioning. A separate trusted recovery workstation and offline encrypted key
copies remain necessary; a second directory on this Ubuntu machine is not a
separate recovery machine.

As checked November 7, 2026, published B2 pay-as-you-go pricing starts at
$6.95/TB-month, with the first 10 GB storage free and free egress up to three
times average monthly storage, then $0.01/GB. Storage-only estimates before free
allowances are about $0.00695/month for 1 GB or $0.0695 for 10 GB. Class A/B/C
API calls are free; Class D has separate allowances/pricing. Confirm the chosen
region/account terms, taxes and Railway export egress before approval. No paid
resource was created. Storage usage depends on ciphertext size times retained
snapshots, not database-volume capacity. An optional $1/month alert is monitoring,
not a provider hard cap. Measure a synthetic sample and then an approved real
snapshot; do not claim a measured live backup size.

Sources: [B2 pricing](https://www.backblaze.com/cloud-storage/pricing),
[application keys](https://www.backblaze.com/docs/cloud-storage-application-keys),
[key capabilities](https://www.backblaze.com/docs/cloud-storage-application-key-capabilities),
[Object Lock](https://www.backblaze.com/docs/cloud-storage-enable-object-lock-with-the-native-api).

Propose 30-day default GOVERNANCE retention, unique UUID snapshot prefixes and
no automated deletion during the pilot. This incurs continuing storage charges
while objects are retained; the owner must approve duration and override custody.
COMPLIANCE lock is not proposed because it removes even the owner's early-delete
option. Bucket administration/retention override stays offline with the owner.
Use a bucket/prefix-restricted expiring writer with `writeFiles`, no delete,
read, bucket/key management or governance bypass; a separate recovery identity
gets only `listFiles`/`readFiles`. Exact API/SDK discovery capabilities must be
reviewed in a synthetic permission-denial test before installation, never widened
by fallback. Separate owner-verification access may inspect retention metadata.
Never use an account/master key in the exporter. Confirm private bucket/object
access and anonymous denial, denied delete/overwrite/retention changes, pinned
object version/file IDs and HTTPS before moving a live snapshot. Unique names
and locked retained versions protect originals; they do not make a mutable
"latest" pointer trustworthy.

## Keys and explicit owner participation

Keep the existing OpenPGP AES-256/OCB profile. One Ed25519 certification primary
with a cv25519 encryption subkey receives the backup. A **different** Ed25519
certification primary/signing subkey signs its canonical manifest. Four full
primary/subkey fingerprints must be independently approved; two generated keys
alone do not establish independent custody. No wallet, GitHub, operator, API or
registry key is reused.

The owner must:

1. Choose a trusted separate workstation with encrypted working storage, maintained
   compatible GnuPG/PG18, no terminal recording/cloud synchronization, and a named
   independent public-certificate reviewer. If unavailable, arrange a supervised
   trusted recovery workstation; do not waive this requirement or create substitute
   production keys through the coding agent.
2. Perform the existing interactive [8H key-enrollment handoff](milestone-8h-key-enrollment.md)
   privately after approving generation. Passphrases use trusted pinentry only.
   Keep decryption keys off Railway, GitHub, this model-connected workspace and
   backup/object storage. Keep signing and recovery-decryption key homes separate; the same approved
   independent workstation can host those isolated homes. Production signing is
   owner-mediated initially, not an unattended CI service.
3. Place encrypted recovery key/revocation copies on two separately stored media,
   with passphrases accessible through a separate approved recovery procedure.
   Prove decryption using a recovery copy after removing access to the original
   key home. Confirm the supported OCB implementation on that workstation.
4. Export **public certificates only** to public-only review/encryption homes.
   The reviewer verifies full primary and subkey fingerprints, intended identities,
   capabilities, expiry, revocation, algorithm/profile and certificate-file SHA-256
   through an independent channel. Sign/date a non-secret review receipt binding
   the four pins, certificate hashes, account/destination, verifier and purpose.
   A fingerprint received alongside an untrusted certificate is not authorization.
5. Approve expiry/renewal and revocation policy; retain old decryption keys and
   authenticated historical evidence. The current helper rejects expired/revoked
   keys; renew/review before expiry and test archival validation deliberately.
6. Select the B2 account, region, bucket/prefix, retention and recovery destination.
   Approve creation and scoped credentials separately; enter them only through
   private approved interfaces. Never paste private keys, passphrases or access
   secrets in chat. Only non-secret locations/public-review evidence are needed.

`config/staging-recovery.example.json` is a separate fail-closed proposal with
null production pins/destinations and every production permission false. It does
not change the earlier primary-database plan. No real certificate was read or
new operational trust installed. Enrollment must verify that encryption/verifier
homes contain **public keys only**, with 0700 directories/0600 files and no symlink
ancestry; existing crypto helper enforces these local properties.

## Snapshot, manifest and integrity procedure

After separate explicit export/transfer authorization, identify only the Railway
project, staging environment, PostgreSQL service and `supa_staging` cluster before
any export. Use the minimum existing authorized backup connection with full TLS
hostname/CA verification. Do not expand runtime permissions, export other databases,
copy the PGDATA volume, or use the protected primary.

Use a maintained PostgreSQL 18 `pg_dump` compatible with server 18.6; custom format
provides a consistent logical snapshot without stopping the source. Require bounded
lock/connection timeouts and fail on errors; no schema/data filters that silently
omit private feature schemas, avatars or ledger/history. The existing 13 migration
bytes/checksums remain untouched. Record current extensions, server/client versions,
encoding/locale, schema inventory, function ownership/search_path, restricted role
attributes/memberships, ACLs and data/sequence integrity in the **encrypted inner
inventory**. Export only reviewed role definitions without password verifiers;
never blindly replay `pg_dumpall` globals or transport live connection secrets.

The current crypto primitive caps plaintext at **16 MiB**. Measure the complete
bundle and fail if larger; reviewed streaming orchestration is not implemented.
No large archive may silently bypass the bound. A real export orchestrator is
still required and must be separately reviewed/authorized; the synthetic runner
is deliberately not that tool. Quarantine any temporary plaintext exclusively on
encrypted owner-controlled private storage (RAM alone may swap); never under Git,
cloud sync, logs or retained application artifacts. Future private orchestration
must scrub inherited credential/proxy settings and never place passwords in argv.

Inner inventory: exact dump SHA-256 and byte count, approved source identity,
migration/extension checksums, sanitized role/owner mapping, schema/data/ACL
fingerprints and read/restore instructions. Sensitive configuration, records and
plaintext hashes stay inside the encryption envelope. Maintain the expected
snapshot UUID and signed-manifest digest in an independent approved inventory.
The outer manifest uses the existing helper's strict canonical fields only:

```text
version, snapshot_id, created_at, filename, bytes, sha256,
profile, recipient_primary, recipient_subkey, signer_primary, signer_key
```

It contains ciphertext length/hash and public pins, no rows, passwords, private
keys, internal hostnames or original dump paths. The template is non-secret,
not an approved snapshot. Owner signing uses the dedicated pinned subkey and
SHA-256-or-stronger detached signature. Recheck encryption status AES256/OCB,
complete process success and the public-only key constraints. Upload only the
ciphertext, canonical manifest and detached signature to unique objects, recording
exact immutable version IDs and retention metadata. Retain full verification
receipts privately, without credential values. Do not overwrite a prior snapshot.

For consistency comparisons, use the same exported MVCC snapshot for the dump
and row/schema fingerprint or an explicitly quiesced approved snapshot window;
sequence allocation is not fully MVCC transactional and needs a reviewed policy.
A simple fingerprint before/after a busy live dump is not sufficient. Do not pause
or change the source automatically. The synthetic fixture is quiescent and
checks its fingerprint on both sides; this is not a live concurrency certificate.

PostgreSQL references: [logical dumps](https://www.postgresql.org/docs/18/backup-dump.html),
[pg_dump](https://www.postgresql.org/docs/18/app-pgdump.html).

## Independent restore acceptance

1. On the separate trusted recovery destination, use a separate read-only storage
   credential to download the **pinned** ciphertext/manifest/signature versions.
   Verify HTTPS, object length/hash, retained version/lock metadata and signed
   manifest against independently approved pins/snapshot identity before decrypting.
2. Fully decrypt to private encrypted quarantine using the recovery-key copy.
   Require exit success, explicit AES256/OCB status and verified inner inventory.
   GnuPG may emit provisional plaintext on authentication failure: never pipe
   decryption directly to `pg_restore`; never restore partial output.
3. Restore only to a newly initialized, uniquely identified, empty PostgreSQL 18
   cluster/database. Verify socket-only listeners, owner-only paths and no live
   credentials, external egress, extensions or connections to Railway/protected DBs.
   Inspect trusted dump TOC and sanitized roles first; restore with exit-on-error
   and single transaction, no `--clean`, no overwriting and no unreviewed global SQL.
   Recreate approved nonprivileged roles without real passwords; review object
   owner mapping explicitly, especially SECURITY DEFINER functions. No blanket
   runtime table writes or restored SUPERUSER/CREATEROLE/BYPASSRLS memberships.
4. Compare all user schemas/tables/rows, column/constraint/index/sequence metadata,
   migration checksums, avatars, ownership/access rows, restricted grants and
   function owner/search_path. Check permission denials, paused/zero-authority agents,
   source independence and recovery after stopping/restarting **only the clone**.
   Then run exact-image Factory/API/frontend compatibility/security checks there.
5. Issue a signed non-secret restore receipt binding source/snapshot ID, cipher hash,
   private destination identity, tool versions, checks/results and time in UTC.
   Keep detailed catalog/data evidence encrypted. Only a successful independent
   real-snapshot restore can mark Railway recovery verified. A local fixture or
   native volume backup is insufficient. Stop on failed checks without fallback.

Proposed pilot RPO is at most 24 hours plus an extra prechange snapshot; RTO is
**unmeasured**, to be established by the real drill. There is no PITR/WAL archiving
claim or newly running backup scheduler. Owner approval is needed for any later
schedule and retention/deletion policy. Failed/stale backup evidence blocks new
persistent pilot records; it never triggers automatic production restoration.

## Synthetic validation and remaining gate

Run `TMPDIR=/dev/shm python3 scripts/test-staging-recovery.py --synthetic-only`
with the existing vendored PostgreSQL 18 tools. This opt-in script accepts no DB
URL, live target, supplied key home or production dump. It initializes two new
socket-only local clusters and uses the existing cryptographic suite's disposable
one-day fixture keys. It executes all 13 original migration SQL files in a new
fixture, then the three reviewed feature extensions; this is not a fabricated
SQLx deployment history or modification to any existing migration.

It checks custom dump creation, complete encryption/signature/inner-hash validation,
tamper rejection before decryption, a fresh fixture decryption-key home, restored
schema/rows/constraints/indexes/sequences and function ACLs, ownership filtering,
function-only grants/direct-write denial, deliberate extra-grant detection,
avatar-byte integrity, duplicate prevention and restored-cluster restart persistence. The source fixture is stopped before restoration. Temporary
keys/data remain private and are removed after their own processes stop; no key
material is retained in the sanitized result. Existing registered clusters are
checked unchanged. Additional existing crypto/disk-guard suites cover malformed
manifests, wrong signers/recipients, expired keys and same-disk rejection.

Results: the complete PG18.3 synthetic rehearsal passed, along with all 15
existing cryptographic regressions and five independent-disk guard tests. The
restore comparator canonicalizes ACL privileges rather than depending on array
order; a deliberately added UPDATE grant must be detected before it is revoked
in the fixture. Owners, grantors/grantees, grant options and function settings
remain part of the comparison. JSON proposal and Python compilation checks pass.
Evidence: ignored `artifacts/staging-recovery-preparation/synthetic-result.json`.
This fixture uses the same host and synthetic keys; it does **not** prove independent
storage, human custody, a cloud transfer, compatibility with the live server's exact
minor release, or recoverability of the actual Railway database. No real recovery
key was generated, and no source credentials or sensitive backups were accessed.

Next: owner completes account/destination and independently reviewed key custody,
then separately approves exact credential installation, one narrowly scoped live
export/encrypt/sign/upload and independent download/decrypt/restore operation.
The earlier broad recovery request does not override the latest explicit ban on
live export/transfer without separate authorization. Factory rollout remains gated;
no migrations, identities, publication or application deployments are included.
