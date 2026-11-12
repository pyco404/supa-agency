# Hosted synthetic reasoning release

Reuse the supervised task runtime, ControlledProvider adapters and the explicit
`agent-reasoning.sql` upgrade. The hosted demonstration uses only
`mock / synthetic-incident-v1`: one request, 2,000 input / 256 output mock tokens,
zero charge, no tools, no autonomous workers. Mock output is visibly labelled;
it is not evidence of a real model call. Existing media and navigation stay intact.

## Approval and accounting

Task approval binds stored input SHA-256 and expires. It does not authorize paid
inference. Real dispatch additionally requires `SUPA_SUPERVISED_SPENDING_APPROVAL`,
a private server-side JSON receipt containing owner_id, task_id, input_sha256,
budget_id, project, model, expires_at, per_task_microusd and cumulative_microusd.
The receipt must match the exact persisted task and real plan. Its expiration is
no more than ten minutes away. The existing one-task/owner/provider gates remain
required. The database permits one lifetime reservation per budget UUID: neither
restart nor cancellation releases an ambiguous request for reuse. Per-task and
cumulative ceilings must cover the pinned 378-microUSD worst-case reservation.
This release installs no paid approval, model credentials or paid-provider flag.
Zero is therefore the effective hosted paid-inference ceiling.

Results remain awaiting_review. Accepting the exact result checksum completes
only that task and approves only the hypothesis memory explicitly requested in
its input. Stop rejects the proposal and retains no memory. Owner revocation,
expiry, cancellation and failed validation cannot retain a model result as a
completed task. Evidence quotations do not establish causation.

## Schema and recovery

Apply agent-reasoning.sql once after agent-supervised.sql, as the existing
migration role. It adds only agent_supervised.inferences and replaces bounded
command/view functions. Runtime retains only schema USAGE and command EXECUTE;
no raw task table, billing-table or elevated role access is granted. Original
migrations, deterministic tasks, memories and protected financial records remain.
On failure restore previous API/frontend code and disable the reasoning UI;
preserve additive tables and all task/audit records. Never silently down-migrate.
Reserved interrupted requests require inspection and Stop, never automatic replay.

## Gates

Before rollout require disposable PG18 transaction/ownership/restart tests,
loopback provider fault injection, desktop/mobile mock journeys, frontend build,
TypeScript, lint, full vulnerability and embedded-secret scans, one-file history
and exact source/image verification. Preserve every MEDIUM/LOW/UNKNOWN finding.
The prior API workflow conclusion remains failure because job-token REST package
metadata omitted repository association; owner-authenticated independent proof
passed. GitHub's repository-scoped GraphQL PackageType interface offered no GHCR
container proof during the November 12 audit. Do not substitute OCI labels for
repository association or mark missing metadata verified. Independent owner proof
is mandatory before accepting a privately published candidate with that omission.

Independent encrypted off-platform restore remains required before real users or
sensitive production records. X worker, wallet, trading, registration and paid
inference stay disabled/unchanged. A real gpt-6-luna evaluation requires a fresh,
separate explicit authorization after the hosted mock demonstration passes.

The hosted mock UI and task-submission proxy require
`SUPA_SUPERVISED_REASONING_ENABLED=true`; unset or false keeps reasoning creation
disabled without changing existing deterministic tasks. Enable only after the
reasoning schema and compatible API have passed their gates.

Current reviewed reasoning SQL SHA-256: `5c76b1ebe1133d5efd92063887dc0044403709e355b0c85649cc265a3320dac4`. The view preserves
the exact deterministic task response shape and exposes inference metadata only
for reasoning tasks; a PostgreSQL regression checks this compatibility. The
earlier checksum in the local preparation document is historical.
