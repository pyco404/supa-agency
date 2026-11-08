# Isolated Railway PostgreSQL logical recovery — verified

On November 8, 2026, one approved PostgreSQL custom dump was restored once into
an isolated, temporary Railway PostgreSQL 18.6 instance. Logical recovery and
persistence across one recovery-instance restart passed. All temporary resources
were subsequently removed. No native snapshot restore or swap was performed.

## Source and backup identity

- Project: `99648d2a-69a1-4d8c-98d0-7f2a26543c9c` (Supa Agent).
- Source staging environment: `ff0b2092-e42b-4a89-ae25-441ab95d92d8`.
- Source `supa-postgres`: `77352ca9-435e-4870-b092-58365310c0f3`.
- Source deployment: `db546a74-4038-4536-b450-d93e78362808`, still SUCCESS.
- Source database: `supa_staging`; cluster `7704432314739101731`;
  PostgreSQL 18.6; source observations used verify-full TLS 1.3 and read-only SQL.
- Bundle: 899,072 bytes, SHA-256
  `b41dc02aa1c76f4acd1d4c0c57cf542f73de3c575c07d62e1e63a03f705eba82`.
- Embedded custom dump: 231,429 bytes, SHA-256
  `5c153422e0184c946534b9e2e478aaa084d0ce89bc5bf95fc6a4cb0d33045842`.

Both checksums were verified before restoration and again against the retained
source-volume backup after cleanup. The original bundle remains private with
0700 directory / 0600 file permissions. No second export was performed.

## Temporary recovery identity and isolation

- Environment: `ff1981d9-31e5-4128-8861-88a80fd611bd`.
- Service: `26c58d7b-c186-4cef-a67f-4ef340d2a723`.
- Deployment: `7fa8f4b8-114c-4064-9f15-2b283de6be9c`.
- Volume: `396645ce-3428-4e3c-8242-1450af457879`.
- Database: `supa_recovery`, with its own cluster identity and storage.
- Runtime: official digest-pinned PostgreSQL 18.6 Debian image
  `docker.io/library/postgres@sha256:02fb3245e038f33536cf13bc6c4e59f21d07b0cea3c1fe1c07c6e8aa8f083bcc`.
- Enforced container limits: 0.5 vCPU and 500,000,000 memory bytes.
- No service/custom domains or TCP proxies. PostgreSQL listen_addresses was
  empty: Unix sockets only, with 0700 socket permissions and SCRAM authentication.

A fresh recovery administrator credential was installed only in the temporary
service. No source password, role verifier or application credential was copied.
Source role names were recreated as passwordless NOLOGIN, NOSUPERUSER,
NOCREATEDB, NOCREATEROLE, NOREPLICATION and NOBYPASSRLS identities. This deliberately
preserves object ownership and restricted object grants without cloning source
login authority or administrator superuser attributes. There were no memberships
for the restored source identities. Temporary export-role ACLs present in the
backup were revoked in recovery and their placeholder roles removed before the
comparison, matching the approved post-export source evidence.

From the recovery environment, `supa-postgres.railway.internal` did not resolve.
Together with separate environment networking, passwordless NOLOGIN source-role
placeholders and absent source credentials, this prevents restored identities
from authenticating to the live database. No live-source write was attempted as
a test.

## Restore, permissions and restart acceptance

The one restore used pg_restore with exit-on-error and a single transaction,
with a 90-second process timeout. It completed successfully. All 58 logical
inventory records matched the export snapshot:

- All 50 table row counts and SHA-256 data fingerprints.
- All 13 successful migration records and their exact SHA-384 checksums.
- Columns, constraints, indexes, triggers and function definitions.
- Schema/relation/function ownership and normalized ACL privileges.
- The complete restricted-runtime table privilege matrix.
- All six sequences, including both last_value and is_called from dump SETVAL
  evidence, as well as the sequence catalog comparison.

The runtime identity could read the approved synthetic agent. Eight actual
permission-denial tests returned SQLSTATE 42501: agent writes, financial-audit
writes, migration-history writes, public table creation, schema creation,
temporary-table creation, migration-role escalation and self-grant escalation.
These tests reduced session_user as well as current_user, preventing the recovery
administrator's session authority from invalidating role-escalation checks.

One Railway restart targeted only the recovery deployment. The verifier required
a changed PostgreSQL postmaster start time before accepting readiness, then
repeated the complete snapshot/data/sequence/ownership/grant comparison. All
comparisons passed again. No failed restore, deployment or restart was retried.

## Source integrity and cleanup

Final read-only verification completed at 01:49 UTC on November 8. The source
matched its pre-drill logical fingerprint across tables, sequence states,
migrations, schema, functions and ACLs; source role attributes also matched.
Normal API heartbeat timestamps were explicitly excluded from the source-to-source
data fingerprint, as in the approved export evidence. No physical/byte-for-byte
unchanged claim is made for a running PostgreSQL volume.

PostgreSQL, API and frontend remained SUCCESS with their same deployment IDs,
image sources, replica settings, domain settings and proxy settings. No source
restart, database mutation, migration, grant change, credential change, DNS change
or application deployment was performed.

Cleanup stopped the recovery deployment, deleted its volume, service and
environment, and removed the private local transfer copy. The final project
inventory matched the pre-drill inventory: temporary environment and service were
absent. Temporary recovery credentials disappeared with those resources. Original
validated backup material and sanitized evidence were retained. The provisioning
to cleanup interval was 235 seconds.

## Costs, evidence and decision

Workspace usage was reported as $22.35773232480611 before and after the drill,
against the existing $60 hard limit, with isOverLimit=false. This unchanged
aggregated observation is not evidence that billable temporary resources were
free. An exact attributable incremental charge was not verified; metering/billing
may lag. No recurring recovery resource remains.

Sanitized local evidence is in the ignored
`artifacts/railway-logical-recovery-drill/isolated-recovery-intent.json` and
`isolated-recovery-final-verification.json`. These receipts are unsigned and
contain no dump contents, passwords or recovery secrets. The approved backup
itself remains on the private source volume; its separate recovery copy was
removed after verification.

**GO for the synthetic-only staging integration recovery gate.** This result
does not authorize migrations, synthetic identity installation, API/frontend
rollout or feature activation; those still require their scoped approval.

This is verified Railway-hosted logical recovery, not tested native-snapshot
recovery or independent encrypted off-platform recovery. Railway account/provider
loss and loss of the source volume remain outside this drill's protection.
Independent encrypted off-platform backup, independent key custody and a verified
off-platform restore remain mandatory before real-user onboarding or sensitive
production records. Real registration, agent execution, paid inference, trading,
wallet signing, token launches and X publishing remain disabled.
