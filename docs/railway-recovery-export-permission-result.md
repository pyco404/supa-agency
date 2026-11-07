# Staging export permission verification and stopped logical recovery

Date: November 7, 2026. One authorized source export attempt; no retries, restore,
native-snapshot restore, source restart or recovery provisioning. This is **not**
successful logical recovery. Synthetic-only staging integration remains NO-GO.

## Target and preflight

Project `99648d2a-69a1-4d8c-98d0-7f2a26543c9c`, staging environment
`ff0b2092-e42b-4a89-ae25-441ab95d92d8`, PostgreSQL service
`77352ca9-435e-4870-b092-58365310c0f3`, database `supa_staging`, cluster
`7704432314739101731`. Source PostgreSQL/client version 18.6. Fully verified
CA/hostname TLS 1.3 authenticated connections passed. The original private image
and deployment `db546a74-4038-4536-b450-d93e78362808` remain unchanged.

Read-only review verified all 13 SHA-384 migration checksums, 50 table names and
owners, six sequences and two user schemas (`public`, `epoch_guard`). No table
uses RLS, so no RLS bypass was required or authorized. Existing runtime DML grants
match the historical bootstrap matrix. Public database privileges include
CONNECT only; CREATE/TEMPORARY are not inherited from PUBLIC.

The approved synthetic agent matches the canonical reviewed JSON exactly, is
paused with zero balance, and has empty capabilities/memory. Comparing JSONB text
hashes directly with compact canonical JSON hashes is invalid because formatting
and key ordering differ; the review uses JSONB equality to approved bytes whose
SHA-256 is verified locally. All other original table counts match the approved
bootstrap, except three expected API health records. Those records have kind=api,
enabled=false and no task ticks/errors. Source code registers a process health
record independently of scheduling; these are not fabricated agent activity or
new customer data. Ordinary heartbeat timestamps are allowed to advance.

Three native backups remain non-expired; availability is not proof of restore.
Workspace usage before and after the attempt was reported as $22.153033849454445
against the existing $60 hard limit, isOverLimit=false. Existing service resource
limits, public-origin/authentication configuration and disabled execution flags
were preserved. No new credentials for existing services were installed.

## Temporary identities and effective permissions

Created only:

- `supa_recovery_export_134a8fc85786`: NOLOGIN, no elevated attributes or ownership,
  USAGE on the two reviewed schemas and SELECT on exactly 50 tables/six sequences.
- `supa_recovery_login_134a8fc85786`: restricted NOINHERIT login, two-connection
  limit, two-hour expiration, private random password, membership with SET=true,
  INHERIT=false and ADMIN=false. Read-only/timeout/search-path defaults were scoped
  to this new login in `supa_staging`.

The login privately authenticated and assumed the NOLOGIN role using SET ROLE.
All 50 table and six sequence read privileges passed; no table writes, sequence
USAGE/UPDATE or existing elevated-role membership was granted. Eight actual
SQLSTATE 42501 denials passed in rolled-back read-write transactions: agent
UPDATE, financial-audit DELETE, public-table CREATE, schema CREATE, TEMP-table
CREATE, membership grant, migration-role impersonation and protected approval
function execution. Using read-write transactions for denial tests proves ACL
rejection rather than merely relying on a read-only transaction error. No
sequence-increment operation was invoked. No existing role or credential changed.

Passwords were generated in process memory and sent only through private SSH
stdin, never command arguments, source files, reports or chat. The admin session
suppressed statement/error-statement logging before secret-bearing role creation;
server-wide logging configuration was not changed. No password/verifier was
retained in the evidence bundle or repository.

## Single export failure

The shared repeatable-read snapshot and snapshot inventory completed, then the
single pg_dump custom-format attempt failed at `single_export`, exit code 1.
The shell imposed a file-size ceiling and complete bundle size would also have
been checked against 16,777,216 bytes before transfer/restore. No successful dump,
complete bundle, accepted size or backup SHA-256 exists. Do not infer size
compliance from the physical database size or from the failed partial file.

The wrapper retained stage/exit information but discarded the underlying client
diagnostic before writing its sanitized result. Therefore the exact pg_dump
failure is **unconfirmed**; do not label it a permissions, version, networking or
size failure. A bounded read of failure-time server logs found the eight expected
permission denials and no additional error in the retained categories. Both the
live pg_dump client and server identify as 18.6. Local diagnostic retention was
improved to preserve known bounded client-error categories without raw SQL,
credentials or data. The existing single-attempt intent prevents accidental reuse;
no additional export was executed to test the recorder.

The generated orchestration was subsequently run successfully against a new,
socket-only local PostgreSQL 18.3 fixture, including shared snapshot, restricted
export, bundle creation and cleanup. This narrows the issue but does not reproduce
or explain the live 18.6 failure, nor certify the live backup. No live fallback,
credential expansion or deployment was attempted.

## Independent cleanup and source checks

The failure exit handler revoked the temporary membership, table/sequence SELECT
and schema USAGE grants, then removed both temporary roles and the partial backup.
A separate verified-TLS read independently confirmed zero remaining temporary
roles and sessions. A separate filesystem check confirmed the uniquely created
source temporary directory is absent. No temporary recovery environment, service,
volume or credentials were created; no recovery instance needs deletion.

Post-attempt read-only comparison matched the pre-attempt database/cluster,
migration history, table/sequence inventory, table owners, original role attributes,
database/schema ACL metadata, function definitions/owners/ACLs, runtime DML matrix,
approved fixture checks, table counts and API health summary. This is the verified
logical comparison scope; it is not a full byte-identical physical comparison or
a completed shared-snapshot restore comparison. Normal API heartbeat timestamps
were not frozen, and the source API was never paused or restarted.

Control-plane verification confirmed the exact original PostgreSQL, API and
frontend deployment IDs/images still SUCCESS. No DNS, protected financial record,
API/frontend setting or existing service credential was modified. Real
registration, workers, inference, financial execution and publishing stay disabled.

Additional recovery-instance compute/storage charges: none, because none was
allocated. CPU/traffic from reads and the failed export on the existing source
is not separately metered; an exact total incremental invoice amount cannot be
claimed from unchanged rounded/cached workspace usage.

## Local validation and next action

Reusable local helpers: `scripts/recovery-export-role.py` and
`scripts/recovery-logical-inventory.py`. `scripts/test-recovery-export-role.py`
passed against a disposable socket-only PG18.3 cluster: exact 50/6 inventory,
restricted complete reads, eight denials, sequence-write denial, original normalized
ACL restoration and removal of test roles. Full orchestration also passed locally
without source TLS/identity claims; its source guard was replaced only in the
isolated fixture, never in the live controller. Temporary fixture clusters/files
were stopped and removed. Secrets scanning passed; each tracked file has its own
Conventional Commit. No push is authorized or performed.

Next requires review of the bounded client-diagnostic correction and separate
approval for one instrumented live export/recovery attempt under the same temporary
identity scope, 16 MiB cap and cleanup. Do not repeat the old intent or automatically
retry a failed operation. Do not increase privileges without evidence. No new
application image or backend deployment is required to diagnose this export.

Evidence is private, ignored metadata under
`artifacts/railway-logical-recovery-drill/`, including source comparison,
permission/cleanup result and final control-plane checks. No database contents,
private keys, passwords or password verifiers are retained there. Independent
encrypted off-platform recovery remains mandatory before real-user onboarding or
sensitive production records. Native Railway snapshots were neither restored nor
certified by this logical-drill attempt.

## Instrumented attempt — confirmed timeout launcher incompatibility

The owner subsequently authorized exactly one instrumented staging-only export
attempt, with no restore or recovery provisioning. Read-only preflight again
passed: exact source identity, 18.6 client/server, full TLS 1.3 verification,
original migration checksums and runtime grants, canonical synthetic fixture,
50 tables/six sequences, three non-expired native backups and prior cleanup.
Observed workspace usage: $22.170161662042656/$60, not over limit. No service or
execution configuration changed.

Instrumented attempt: November 7, 2026, 21:50 UTC. Temporary identities were
`supa_recovery_export_535434b69d32` and `supa_recovery_login_535434b69d32` with the
same reviewed minimum grants, expiration, connection limit and defaults. Eight
live expected permission denials passed, as did effective table/sequence read
checks and shared-snapshot inventory reads. The dump launcher exited 1 and
reported `option_rejected`; **pg_dump itself was never launched**.

The root cause is now confirmed: Railway's exec environment resolves `timeout`
to BusyBox, whose parser rejects the GNU-only `--signal=TERM` option. The
controller called `timeout --signal=TERM 90s pg_dump ...`. A harmless read-only
process diagnostic, `timeout --signal=TERM 90s true`, reproduced the exact same
291-byte diagnostic fingerprint:
`b84cd5feee719240346bd6ba93382ee64cfb91fa35a81fbb54e151abac1fd8ea`.
This is an **error diagnostic hash, not a backup checksum**. No SQL, credentials,
paths or data from the client error were retained. The previous attempt used the
same incompatible launcher; its original raw diagnostic was not retained.
No connection-limit error was found in the historical categories inspected;
the reviewed connection limit was not increased.

Minimum correction: `timeout -s TERM 90 pg_dump ...`. Harmless `true`/`sleep`
probes inside the same live exec environment verified short-option success (0)
and actual deadline termination (143). The equivalent GNU launcher tests passed
locally (success 0, deadline 124). Both nonzero timeout results remain failures;
no timeout, read-only snapshot, permission or size requirement was relaxed.
The fixed prefix is centralized in `recovery-export-diagnostics.py` and used by
the private controller; the former controller is preserved as failure evidence.
The prepared corrected controller SHA-256 is
`e157540659553b9c283652e1b65f582a230f96597fb009f2720d99a973268dc8`.
Its used intent still blocks accidental rerun. No corrected live export occurred.

Diagnostics now capture the child operation/status and both output channels in
memory, then emit only allowlisted categories, byte counts and fingerprints.
Protocol parsing rejects arbitrary category/stage strings, URLs, paths and secret
values. Six deterministic tests passed. The actual generated controller passed
three socket-only disposable PG18.3 integration fixtures: successful dump/shared
snapshot/TOC/readback/bundle, denied export, and timeout. Failure cases verified
role and partial-file cleanup and absence of private fixture strings in output.
Local PG18.3 testing does not claim a successful live PG18.6 export or restore.
The prepared successful path also requires 50 TABLE DATA and six SEQUENCE SET
entries, full archive decompression to /dev/null, embedded-dump SHA-256 agreement,
and the complete 16,777,216-byte bundle cap before acceptance.

Independent post-attempt verified-TLS queries confirmed zero temporary roles and
sessions, and the uniquely created source directory is absent. Pre/post identity,
history, table/sequence inventory, owners, original roles, database/schema ACL
metadata, function definitions/ACLs, runtime grants, fixture checks and all table
counts/health summaries match. Normal API heartbeats were not frozen. No existing
service was restarted, no credentials rotated and no financial rows changed.

Result: one failed instrumented launcher attempt, zero pg_dump executions in that
attempt, zero retries, no complete backup/checksum/accepted size, no restore,
and no recovery resources or instance charges. The source remains healthy; the
synthetic-only recovery gate remains NO-GO pending actual export and restore.
Next approval is one corrected export attempt with the same temporary identities,
90-second deadline and 16 MiB cap. If successful, verify/retain the private archive
and stop for separate isolated-restore approval. Do not reuse the spent intent or
automatically retry. Independent encrypted off-platform recovery remains required
before real-user onboarding or sensitive production records.
