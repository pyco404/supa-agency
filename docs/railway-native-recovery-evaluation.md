# Railway-only recovery evaluation — November 7, 2026

Read-only assessment. No restore, snapshot creation, restart, export, resource
provisioning, credential change, database mutation or application deployment.
Backblaze setup remains pending. Independent off-platform recovery is still
mandatory before real registration, sensitive production records or real users.

## Live native backup evidence

Verified project `99648d2a-69a1-4d8c-98d0-7f2a26543c9c`, staging environment
`ff0b2092-e42b-4a89-ae25-441ab95d92d8`, service `supa-postgres`
(`77352ca9-435e-4870-b092-58365310c0f3`). Its approved private image remains
`ghcr.io/pyco404/supa-agent-postgres-infra-98c1006fcb1d@sha256:9409009a40c0929052306d068c14bef92f3ecc67662c453540e2c9b95ae18fc8`;
deployment `db546a74-4038-4536-b450-d93e78362808` remains SUCCESS.

Volume `a9c43441-805d-4474-9395-2b596ee63c20` is attached to this service at
`/var/lib/postgresql/data`, capacity 50,000 MB, currently about 1,132.9 MB used.
Volume instance: `2c70c2f9-bee5-4de5-9477-55441189982a`.
Live backup query returned three non-expired snapshots, not merely a schedule:

| Snapshot time (Africa/Lagos, WAT) | Backup ID | Expiry (WAT) |
| --- | --- | --- |
| Nov 5, 18:25 | `5047e325-8097-458b-9df3-9ca7527f1b4a` | Nov 11, 18:25 |
| Nov 6, 18:25 | `03f4e248-d23a-4986-a607-1307cade21f6` | Nov 12, 18:25 |
| Nov 7, 18:25 | `b7c738c3-d3be-4613-8177-051f40ab7269` | Nov 13, 18:25 |

Live schedule: DAILY, cron `25 17 * * *` (17:25 UTC / 18:25 WAT).
The latest snapshot references 1,132 MB; its exclusive size is reported as 1 MB.
These storage figures are not database logical-dump sizes or proof of consistency.
The API exposes no completed/validated-content status field: availability and
non-expiry are verified; successful restore, SQL integrity and TLS are untested.

Private read-only evidence is in ignored `artifacts/railway-native-recovery/`.
No secret variable values or startup commands were retained.

## Snapshot restore is not a non-disruptive clone operation

[Railway volume-backup documentation](https://docs.railway.com/volumes/backups)
states that restore creates a replacement volume, stages a volume swap and
redeploys the source service when applied. The original volume is retained but
unmounted. Backups can only restore inside their original project/environment.
Daily retention is six days; wiping the volume deletes its backups. Incremental
snapshot storage is billed like volume storage.

Live GraphQL schema confirms `volumeInstanceBackupRestore` accepts the source
volume-instance ID and backup ID, with optional HA replica/wipe service IDs. It
has no independent destination environment/service/volume argument. Those HA
arguments are not a supported custom clone mechanism. The official CLI's
`backup restore` implementation commits/deploys the staged source swap and can
include other pending staged changes. Do not invoke it on `supa-postgres` as a
non-disruptive recovery test, even though it creates a fresh physical volume.

No read-only schema evidence supports cloning an existing native snapshot to a
separate environment while leaving the source unchanged. Do not guess that a
new destination volume ID can be paired with another volume's backup. A future
Railway-supported snapshot-fork procedure would need explicit confirmation and
review before approval. No support message was sent or operation attempted.

## PITR and this custom image

[Railway PITR](https://docs.railway.com/volumes/point-in-time-recovery) can create
a new sibling service without changing the source *when a valid archive already
exists*. Enabling PITR first creates archive storage, changes service variables
and redeploys the source; it does not recover an earlier unarchived point.

Current live service configuration has no `WAL_ARCHIVE_*` or `WAL_RECOVER_FROM_*`
variables, no archive bucket reference and no custom start command. There is no
verified managed archive, base backup or recovery window. The CLI's read-only
PITR status command failed parsing environment configuration; a sanitized
category-only diagnostic was retained. Direct GraphQL backup and configuration
observations succeeded. No live archiver/SQL probe success is claimed.

Official [CLI eligibility rules](https://github.com/railwayapp/cli/blob/master/src/controllers/adoption_eligibility.rs)
show the PostgreSQL PITR template accepts Railway's own database repositories,
requires the image entrypoint and a floating major tag, and rejects digest pins.
Our image is a private custom repository pinned by digest. It derives from
Railway's image, so do not infer that pgBackRest is absent; inherited binaries do
not establish eligibility or correct archiving in this exact frozen build.
Managed PITR is not a supported drop-in for the current approved image. Switching
images/pinning policy or self-managing archival would require source-service
changes and new validation, outside this request. No change is recommended just
to simplify this synthetic pilot's first restore drill.

## Minimum safe Railway-only option for synthetic staging

There is **no verified non-disruptive native snapshot test-restore path** for
this service today. Keep the existing daily volume backups as supplementary
rollback coverage, with their in-place restore implications understood.

Recommend one separately authorized **logical backup/restore inside Railway**:

1. Create one uniquely named recovery environment and one private temporary
   PostgreSQL service/empty volume using the same approved immutable image. No
   frontend, API, workers, public domain, TCP proxy or registration service there.
   Bound compute to at most 0.5 vCPU / 512 MiB and authorize a one-hour test window.
   Additional temporary compute/volume charges apply under the shared $60 limit;
   recheck live capacity before provisioning. No fixed cost or hard test-budget
   cap is claimed. Cleanup/removal needs an explicit scope limited to new test IDs.
2. Configure only the clone's minimum temporary identities and correctly named
   TLS trust privately, under explicit approval. Never rotate the original
   credentials, inherit application secrets wholesale or reuse an original live
   DATABASE_URL on the clone. A physical snapshot would also contain original TLS
   private keys/password verifiers; it is not credential-isolated merely because
   the volume or service is new.
3. After separately approved staging-only export, use maintained PG18 tools and
   the existing authorized source connection with full TLS verification to make
   one consistent custom-format dump of **supa_staging only**, no role password
   verifiers or other databases. Keep it in private encrypted Railway storage;
   cross-environment private networking is isolated, so prepare an authenticated
   Railway control-plane/SSH transfer with complete hashes rather than opening
   a public source endpoint or pretending cross-environment DNS connects directly.
   No workstation export or B2 upload is part of this proposed scope.
4. Restore into the new empty clone with reviewed role/owner mapping, retained
   ACLs and exit-on-error transaction semantics. Compare schema/data inventory,
   all 13 migration checksums, fixture/paused-zero-authority state, restricted
   grants/function owners/search_path, sequences and constraints at the snapshot
   boundary. Verify authenticated read access/TLS, denied runtime writes and
   cross-owner access, and restart persistence by restarting **only the clone**.
   A successful logical restore does not certify a native volume snapshot.
5. Record exact source/clone IDs, image, snapshot/dump hash, source deployment
   preservation, checks and measured duration. Stop on failures. Do not cut over
   source consumers, copy records back, delete backups or enable any execution.

This is a recommendation, not a provisioned/restore-tested system or a deployed
exporter. It avoids migrating the running database to another image or enabling
PITR just for the drill. Native volume snapshots are not subject to the 16 MiB
OpenPGP helper limit; the proposed Railway-only logical transfer is a separate
reviewed path, not permission to bypass that helper for off-platform backups.

## Limitations and exact remaining approval

Railway-only backups share account/project access, provider availability and
retention/deletion exposure with the source; they do not establish independent
key custody, a portable off-provider recovery copy or recovery after loss of
Railway access. Daily snapshots have a backup-time RPO, not continuous WAL/PITR.
A short-lived clone has its own charges and isolation requirements. Restored SQL
health/security and consistency remain unverified until the actual drill passes.
Synthetic-only acceptance does not authorize real users or sensitive records.

Exact next proposed authorization: one new private Railway recovery environment,
one temporary PostgreSQL service/volume and clone-only credentials/TLS, one
staging-only logical export/transfer/restore, bounded clone-only verification and
restart, and explicit cleanup of only those newly created IDs. Preserve every
original service/volume, image, credential, API/website configuration, DNS and
financial record. Keep real registration, execution, trading, token launches,
paid inference and X publishing disabled. No off-platform data transfer.

If only a native-snapshot restore is acceptable, do not authorize an in-place
restore under the existing no-source-change requirement; first obtain a
provider-supported isolated restore procedure. Keep independently encrypted
off-platform recovery mandatory before onboarding real users or storing
sensitive production records; the existing owner setup remains deferred.
