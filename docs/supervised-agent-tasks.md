# Supervised synthetic agent tasks

Local implementation only. No push, image build/publication, Railway operation,
credential change, live SQL installation or execution activation is part of this
work. Real registration and external/financial execution remain disabled.

## Reused foundations and deliberate boundaries

Factory identities, private configuration, requested capabilities, creator ACLs,
synthetic authentication/deadlines, PostgreSQL transactions and the existing
Next.js session/origin adapter are reused. The Rust `Runtime` now exposes a
separate supervised command method. It reuses `WorkRequest`, `analyze_work` and
`relevant_memories`; it does not invoke the configured provider or economic-job
payment/settlement path.

The generic cycle runtime assumes an active agent and writable agent state, and
existing work-service paths include economic approval/payment concepts. Those
are not appropriate shortcuts for zero-spend Factory agents. Their original
freeze trigger and paused lifecycle remain intact. Task state, results and private
memories live in an additive task schema rather than rewriting frozen agents,
claiming paid delivery, or granting general agent/memory/financial table writes.

The first supported task is deterministic extraction from owner-supplied synthetic
text. This produces a genuine report with line references and literal requirement
coverage. It is not a paid LLM, semantic truth verification, arbitrary code
execution, internet research or proof of real-world goal completion.

## Owner journey and limits

From My Agents, open an owned agent's detail page. Submit a task, desired evidence
phrase and synthetic material. The input and SHA-256 are displayed for review.
Approve that exact input, then Start the approved task. Each Run next step performs
one bounded unit, with no queue, worker, interval polling executor or background
continuation:

1. Inspect the bounded input; retrieve up to four relevant private task memories
   only if the creator requested `read_memory` for this agent.
2. Run the existing deterministic extractive analyzer against the supplied text.
3. Record an assessment and optionally retain an evidence-linked private memory.

Retention requires both per-task consent and the agent's requested `remember`
permission. Retained statements are hypotheses, never promoted to externally
verified facts. They bind to the source task, input checksum and actual audit
event. The existing memory-ranking helper receives an in-memory compatibility
projection whose cycle IDs denote source task IDs; no autonomous cycle is claimed
or persisted. Previous private task memories remain viewable with their results.

The desired phrase uses literal coverage, with the analyzer's existing limitations
shown in the report. Assessments require owner review; they do not silently mark
agent goals completed or fabricate progress, achievements or revenue. A completed
task means its bounded processing finished; the report can honestly be incomplete
when the desired evidence is absent.

Limits: 4,000 UTF-8 bytes of material, 600-character objective, 120-character desired
phrase, exactly three execution steps, 250 ms pure computation check per step,
4-second command-handler timeout, transaction-local 2-second statement / 1-second
lock timeouts, a 120-second wall-clock execution deadline, 10-minute approval and
one-hour task admission window. Pausing/resuming never resets the step ceiling or
execution deadline. One approved/running/paused task per agent; at most 20 stored
tasks per agent and 100 per creator. At most 32 revisions before Stop is the only
remaining command. All steps have zero spending and external-action authority.

Pause/Stop are available between atomic steps. Their requests do not wait for the
advance semaphore. Revision checks prevent a late result from committing after
a concurrent control or revocation wins. Resume requires renewed explicit owner
confirmation within the original approval/deadline, not a fresh unattended lease.
Stopped/completed/failed tasks cannot be restarted. Use a new task and approval.

## Authorization, integrity and recovery

The API requires a named, unexpired Owner credential and the default-off synthetic
Factory and supervised-task flags. The frontend derives its bearer token from the
existing authenticated session; callers cannot provide actor IDs, runtime results,
permissions, external tools or financial settings. Exact-origin JSON writes and
strict request schemas remain mandatory. Public pages have no task endpoints or
private task reads. Unknown/cross-owner task IDs are denied without exposing data.

Every operation checks the original Factory creator and current owner ACL in
PostgreSQL. A shared ACL-row lock serializes revocation against command commits.
The runtime computes outside the final transaction, then the database rechecks
ownership, reviewed input hash, revision, state, permission and deadline.

Task input/identity is immutable. Request IDs, revision CAS, a per-owner advisory
lock and a partial unique active-task index prevent duplicate/concurrent execution.
A lost response is resolved by reading current state or replaying the same command
ID; mutations are never automatically retried. Repeated command IDs with changed
content are rejected. Each committed transition appends an immutable hash-chained
audit event with actor, timestamp, source checksum and result checksum. Results,
memories and command receipts commit atomically; a database error cannot retain
an output while losing its evidence or duplicate-protection record.

Restart recovery is passive: stored pending/approved/running/paused tasks remain
stored. Nothing resumes or runs on process/database startup. The owner must inspect
and explicitly advance within the original deadline, or stop the expired task.
The original 13 migration files are untouched. Existing real-data recovery,
protected accounting and registration restrictions remain in force.

## Local validation

`SUPA_FACTORY_TEST_RAM=1 python3 scripts/validate-agent-factory.py --supervised`
uses a newly created, socket-only disposable PostgreSQL 18 cluster with a guarded
`/tmp/supa-factory-test-*` socket, isolated RAM data/cache and synthetic credentials.
It runs the existing Factory/avatar/browser suite, then supervised API/security and
browser tests, restarts only that disposable database, verifies persisted results,
and removes its own stopped cluster. It never accepts a Railway URL or existing
cluster. Existing registered PostgreSQL clusters and original migrations are checked
unchanged. Logs/screenshots are retained under ignored `artifacts/functional-factory`
and `artifacts/supervised`; synthetic fixture inputs are not real-user data.

Coverage includes two creators, default-off gates, approval and input-hash binding,
owner isolation/revocation, duplicate/concurrent requests, forged payload rejection,
stop, expiry, pause/resume, limits, direct-write denial, audit hashes, atomic failure
rollback, persisted private memory and restart without automatic execution. Actual
Chrome → Next.js → Rust → PostgreSQL journeys cover desktop/mobile creation,
manual task steps, results, memory, refresh, cross-origin denial and sign-out.
Session/adapter tests cover absent auth, origin, role/session boundaries, rejected
caller-supplied results and malformed backend contracts. TypeScript, ESLint and
focused Rust tests run locally. New dependencies are not introduced.

## Verified local result

The final local run passed on disposable PostgreSQL **18.3**, with a real database
restart and cleanup verified. The existing Factory/avatar/browser journey and
restart tests passed, followed by the supervised journey and supervised restart
recovery tests. Six runtime unit tests and eight session/adapter tests passed;
TypeScript, ESLint and formatting of all changed Rust files passed. The whole
workspace formatting check also identifies pre-existing platform-token formatting
outside this change; those files were preserved. Dependency lockfiles are unchanged.
No new container vulnerability scan is claimed; fresh image scanning is a release
requirement below.

Both browser users created an agent, approved a synthetic task, paused/resumed
across refresh, completed three real offline processing steps and viewed the
retained private memory. Script-like supplied text rendered as plain text and did
not execute. The browser recorded zero external requests. Audit checks include
hash-chain linkage and a memory source event belonging to the analyzed task;
missing review fields fail closed at the SQL boundary independently of HTTP
validation. Retained memories were retrieved using the existing relevance helper.

Screenshots: [desktop task result](../artifacts/supervised/desktop-task-result.png)
and [mobile task result](../artifacts/supervised/mobile-task-result.png).
The optional script-looking evidence line is intentional inert security-test input.
Reports/logs are ignored local evidence and are not proposed for Git publication.

New extension SHA-256:
`fb1e55678c46abac829ccdfb536e8863c8e52b6fcf3784ef033918894eccf83e`.
It creates three tables (tasks, commands, events) and helper functions; it does not
modify original migration files or existing runtime grants. Proposed grants, for
separate review and approval only:

```sql
GRANT USAGE ON SCHEMA agent_supervised TO supa_staging_runtime;
GRANT EXECUTE ON FUNCTION agent_supervised.command(uuid,text,jsonb)
  TO supa_staging_runtime;
```

## Controlled staging release requirements

1. Review and separately approve exact application commits and minimum CI source
   pin/test changes. Both API and frontend changed and need fresh validated images,
   full unsuppressed vulnerability/secrets gates and immutable private publication.
2. Review `deployment/agent-supervised.sql`, install after ecosystem-social and
   agent-factory using the dedicated migration authority, and grant the restricted
   runtime only USAGE on `agent_supervised` and EXECUTE on
   `agent_supervised.command(uuid,text,jsonb)`. No raw task/event table privileges,
   generic lifecycle permissions or financial grants. Verify checksum and restore
   boundaries against a current synthetic-only backup before live SQL changes.
3. Separately approve controlled API/frontend replacement and
   `SUPA_SUPERVISED_TASKS_ENABLED=true` on both services. API startup verifies its
   restricted task-schema grants; the flag alone cannot install/repair SQL. Preserve
   Factory/avatar flags, origin, public browsing and all existing capability limits.
4. Recheck the two synthetic identities' deadlines; renew only with explicit
   approval if expired. Perform synthetic acceptance and a separately scoped
   persistence check without restarting PostgreSQL or enabling real registration.
5. On failure, disable the new task feature and preserve additive task/audit records;
   never reverse SQL or erase task history automatically. Real users remain blocked
   until independently verified encrypted off-platform recovery and security review.

No live staging execution is authorized merely by this local implementation.
