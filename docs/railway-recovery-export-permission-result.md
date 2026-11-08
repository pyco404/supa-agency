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

## Corrected export succeeded — November 8, 2026

Owner-authorized approved application HEAD:
`2edfdb73b7866a2e8bf97b60c1a22b1654979fd3`. The working tree was clean and
matched that exact revision before execution. No push was performed. The new
single-use corrected intent preserved every previous failed-attempt receipt.
Only receipt filenames changed from the reviewed corrected controller; the
portable 90-second launcher, read-only snapshot, minimum grants and size gates
were preserved. Executed controller SHA-256:
`6021a3916a482d1e31bbf999e3304e5034fb654c1dbf5f67cd8936e14fb18312`.

Fresh preflight passed on the exact existing project/staging/PostgreSQL service
and unchanged approved image/deployment. PostgreSQL/client tools were 18.6,
verified TLS 1.3 passed, all 13 original migration SHA-384 checksums matched,
and the approved paused/zero-balance synthetic fixture and expected API-only
health records matched. Original roles, table owners, runtime privileges and
schema/function metadata passed review. Three non-expired native backups remain
listed, without any native restore claim. Usage was $22.34291187142247/$60,
not over limit. No existing service, credentials or execution flags changed.
Six diagnostics/launcher tests and disposable PG18 permission/cleanup tests
passed before execution; the secrets scan passed.

**Exactly one corrected logical export succeeded**, completed November 8 at
01:23:05 UTC / 02:23:05 WAT. The restricted login assumed its temporary NOLOGIN
export role. All 50 table and six sequence read permissions passed, no sequence
write privileges existed, and eight SQLSTATE 42501 denial tests passed. The
read-only snapshot holder remained open while both the inventory and pg_dump
used its exported repeatable-read snapshot. Portable `timeout -s TERM 90`
returned dump exit code 0 with no client diagnostic output. No retry occurred.

| Accepted object | Format | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Logical database dump | PostgreSQL custom archive | 231,429 | `5c153422e0184c946534b9e2e478aaa084d0ce89bc5bf95fc6a4cb0d33045842` |
| Complete retained recovery bundle | tar, five reviewed members | 899,072 | `b41dc02aa1c76f4acd1d4c0c57cf542f73de3c575c07d62e1e63a03f705eba82` |

The 16,777,216-byte limit applies to the complete bundle, not merely the dump.
Accepted bundle members: database.dump, snapshot.json, snapshot.sql, before.json
and before.sql. Read-only pg_restore TOC/decompression checks passed without
executing restored SQL. Coverage was 50 TABLE DATA and six SEQUENCE SET entries.
The dump extracted from the bundle matched the original accepted dump SHA-256.
A separate SSH observation recomputed the complete bundle and embedded dump
hashes and measured their sizes independently. No database contents were printed,
transferred off-platform or retained as local plaintext backup artifacts.

The validated bundle remains privately on the existing Railway PostgreSQL
volume, in the unique directory recorded by the private corrected-export intent
and receipt. Directory mode 0700 and bundle mode 0600 were independently verified.
Only recovery.tar remains; loose dumps, SQL/probe files and temporary credentials
were removed. No automatic deletion policy was added while isolated restore is
pending. This same-volume, provider-controlled storage is not an independent
backup destination, independent key custody, off-platform encryption/signature
verification or protection from source-volume loss. No recovery/signing private
keys were generated. The non-secret receipt is unsigned and does not claim them.

The full pre/post logical inventory comparison passed across all source tables,
rows, column definitions, constraints, indexes, sequence states, function
identities/definitions, triggers and normalized object ACLs. Only ordinary API
heartbeat timestamps were excluded from that comparison; the API was not paused
or restarted. Separate post-export read-only review also matched identity,
migration history, table/sequence inventory, owners, original role attributes,
database/schema ACL metadata, function definitions/ACLs, runtime DML matrix,
canonical fixture, table counts and health summary.

After the snapshot and dump sessions closed, the temporary export membership,
SELECT/schema grants and both temporary identities were removed. Independent
verified-TLS SQL confirmed zero remaining temporary roles and zero sessions.
The source remains PostgreSQL 18.6, TLS 1.3, healthy and outside recovery mode.
No migrations, financial rows, existing credentials, API/frontend settings,
DNS or original service deployments were changed. All execution restrictions
remain in place.

Result: **export/integrity/access-cleanup PASS; restoration NOT ATTEMPTED**.
No temporary environment, service, volume or recovery credentials were created.
No native backup was restored. The isolated recovery test and restart-persistence
checks remain pending; successful archive readback is not successful restoration.
The preserved dump includes the snapshot's object owners/ACLs, including the
now-removed temporary export-role grant. The isolated restore must use the reviewed
nonprivileged placeholder/owner-role mapping, isolated credentials and temporary
ACL cleanup; it must not import source password verifiers or broaden source grants.

Next approval: one isolated temporary PostgreSQL 18 recovery drill using the
exact retained bundle hash, controlled private transfer, fresh target/credentials,
reviewed role mapping, schema/data/migration/permission comparisons, clone-only
restart and cleanup limited to those newly recorded recovery resources. No new
export or application deployment is needed if this retained bundle is still
available and hash-matched. Recovery remains unverified until that independently
read restored database passes. Independent encrypted off-platform recovery stays
mandatory before real-user onboarding or sensitive production records.

Private non-secret evidence: corrected-export-intent.json,
corrected-export-result.json, corrected-export-verification.json and
corrected-export-receipt.json under ignored
`artifacts/railway-logical-recovery-drill/`. Actual backup bytes remain only in
private Railway storage. No credentials, rows or private key material enter Git.
