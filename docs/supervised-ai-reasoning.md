# Owner-reviewed supervised reasoning

Local synthetic-only extension of the existing supervised task workflow. No paid
model request, hosted service change, registration or autonomous execution is
part of this implementation.

## What is connected

My Agents reuses task creation, exact input hashes, owner ACLs, approval expiry,
manual Start/Pause/Resume/Stop, the Rust runtime and PostgreSQL audit history.
Reasoning mode uses the existing `ControlledProvider::work` interface,
`WorkProposal` evidence validator, `InferencePolicy` reservation/cost arithmetic
and `TokenUsage`. It does not invoke autonomous cycles or economic settlement.

The currently available provider is explicitly **mock** (`synthetic-incident-v1`).
It returns a structured incident proposal with quoted source observations,
separate interpretations, missing evidence and a suggested follow-up. It is a
transport substitute for local tests, not evidence that a live AI model reasoned
about an incident. The prepared real adapter is default-off and task-scoped; the UI still creates
mock plans only. No credentials are needed for this demonstration.

## Owner journey

1. Sign in with an authorized synthetic creator; select an owned agent in My Agents.
2. Choose Incident reasoning / mock model. Supply the synthetic task, desired
   evidence and incident notes, with optional private-memory retention consent.
3. Review the exact stored input and mock-model plan: one request, at most 2,000
   input and 256 output tokens, no retries/tools and zero cost. Approve, then Start.
4. Inspect input with the first manual step; pause/resume if needed. The next step
   reserves and invokes the mock provider once. No background executor exists.
5. Inspect the returned observations, line citations, uncertainty and usage.
   Refreshing preserves the proposal; the task remains awaiting review.
6. Explicitly accept the exact result checksum to complete the task and retain an
   evidence-linked private hypothesis if consent and Remember permission allow it.
   Stop rejects the proposal without retaining a memory.

Completion records owner review, not proof that the incident cause is established
or an agent goal has been achieved. Goal assessments remain private, provisional
and evidence-linked; original frozen agent goals and financial records are not
silently changed. Existing task-memory retrieval remains local. Stored memories
are not automatically sent to the model.

## Request and result safety

The plan is immutable inside the approved task input. The UI submits the exact zero-cost mock configuration. An explicitly prepared
real plan can instead pin the reviewed provider, model, project, unique evaluation
budget, 378-microdollar ceiling and an absolute expiration within ten minutes. The mock model defines one token as four UTF-8
bytes rounded up, counted over the serialized provider request/response. This
makes local usage deterministic; it is **not** an estimate for any real model.
The real adapter counts the complete instructions, serialized input and output
schema through the Responses input-token endpoint before generation. It never
uses the mock tokenizer for real requests; unavailable/oversized counts fail closed.

`deployment/agent-reasoning.sql` is an explicit upgrade after the unchanged
`agent-supervised.sql`. It adds one private inference-reservation table, a bounded
plan function, an awaiting-review state and upgraded command/view functions.
The existing restricted role retains only schema USAGE and command EXECUTE; it
receives no direct table access or new financial privileges. No original migration
file is modified. Installation is never automatic.

A unique task reservation is committed before inference. Concurrent/repeated
requests cannot generate a second call. Replays must match the original command
body; changed input, confirmation or revision is rejected. Cancellation, pause,
revocation, deadlines and provider errors discard results. Accounting settlement
can record the receipt of an already reserved request after revocation, but cannot
release its output or create memory for the revoked owner. State, result, usage
and hash-linked audit writes are transactional.

There is a 1.5-second mock-call deadline within the default 4-second handler
limit. The separately gated real path has a 25-second total runtime deadline,
30-second API handler, 35-second frontend proxy and 40-second browser request limit. Pause/Stop observations are bounded read-only checks while the call is
pending. A dropped process/response never implies inference succeeded. Reserved
or interrupted calls are not retried, including after restart; inspect and Stop
such a task, then create a newly approved task if needed. Resume is available
before a model request, not a mechanism for repeating an interrupted request.
Result review must occur within the original approval window. No task continues
on startup or in a worker.

## Validation and release boundary

The guarded disposable PostgreSQL 18 runner exercises the unchanged extraction
journey and the reasoning extension. Synthetic browser journeys cover both
creators at desktop/mobile sizes, plan approval, result review, refresh, private
memory and audit evidence. Integration tests cover unsupported observations,
model/token/budget changes, missing/invalid usage, concurrency, duplicate commands,
expiry, cancellation, revocation, transaction failure and restart without replay.

The live website and Railway database remain unchanged. Hosted release needs
separate reviewed source/image validation, private integrity-verified publication,
explicit SQL/grant approval, bounded synthetic identities and controlled API and
frontend deployments. Real registration, paid inference, wallets, trading, token
launches, autonomous workers and X publishing remain disabled.

## Verified local result

The final guarded run passed on disposable PostgreSQL 18.3, including an actual
restart, cleanup and unchanged original migrations/registered clusters. Both
synthetic creators completed the extraction and mock reasoning browser journeys
on desktop/mobile, refreshed the awaiting-review proposal, explicitly accepted
it, and refreshed the persisted result/memory. Browser external requests: zero.
The sample mock request used 74 input and 193 output tokens at zero cost.

The baseline domain/provider/runtime unit suites passed (80 tests). Five focused work-provider
wire tests also passed against loopback mock HTTP; three were normally ignored
by the default suite. Nine session/adapter tests, TypeScript, ESLint, changed-Rust
formatting and inline-script syntax checks passed. No real model or container
vulnerability scan was performed. No new dependencies were introduced.

The baseline mock-only reasoning upgrade checksum was
`f1781fbb761dad7296a6fc4f267d02fc628bd88f4dfb7ddd0b1c30b0d26ebd6f`;
the prepared real-adapter upgrade is reviewed separately below. It has not been
applied to Railway.
The original supervised extension remains
`fb1e55678c46abac829ccdfb536e8863c8e52b6fcf3784ef033918894eccf83e`.
Synthetic screenshots/reports are retained under ignored
`artifacts/supervised/reasoning/`; no credentials are committed.

## Prepared real-model adapter (disabled)

`SupervisedOpenAi` reuses the existing retry-disabled, redirect-disabled HTTPS
client, strict work schema/evidence validator, usage parser and `InferencePolicy`
cost/reservation arithmetic. It does not reuse the larger generic inference
allowance. Its only reviewed model ID is `gpt-6-luna`; a different alias or returned
model fails closed. The official catalog currently supplies no dated snapshot
for this model, so matching the ID does not assert immutable model weights.
Project-specific availability and quality remain untested until an approved run.

Verified 2026-11-08 against the official [model documentation](https://developers.openai.com/api/docs/models/gpt-6-luna):
Responses and structured outputs are supported, with `reasoning.effort=none`.
Standard USD prices per million tokens are input $0.10, cached input $0.01,
cache writes $0.125, output $0.50. At 2,000 input / 256 output, uncached pricing
is $0.000328; the conservative reservation is **$0.000378** allowing cache writes.
These reviewed rates must be rechecked before authorizing the first evaluation.
No regional, Fast, Batch or Flex pricing is assumed.

The [official token-counting guide](https://developers.openai.com/api/docs/guides/token-counting)
requires counting formatting and schemas, not just task text. The adapter sends
the exact same model, instructions, serialized input, reasoning configuration and
strict output schema to `/v1/responses/input_tokens` that it uses for generation.
Generation alone adds `store=false`, `service_tier=default`, `max_output_tokens=256`.
Zero, missing, malformed, oversized or unavailable counts prevent generation.
Returned input usage must match that count. A model/tier mismatch keeps only
unpriced diagnostic counts; its actual cost stays unknown and the reservation held. The generation response must have the
exact model, default service tier, bounded usage and only message output, with
valid line-referenced findings and explicit uncertainty. No tool calls are accepted.
A durable task-state/owner/expiry guard runs again after counting, immediately
before the generation send; revocation during preflight prevents dispatch.
There are at most three external HTTP requests: model metadata, token count,
and **one** generation. Neither metadata/count nor generation is retried.

The existing task reservation is committed before those requests. The upgraded
inference table pins a unique evaluation-budget UUID across all tasks, reserves
378 microdollars and records observed usage/cost separately. Unknown usage is
**NULL, not zero**; the reservation stays held and the attempt cannot be repeated
on refresh, cancellation, revocation, process loss or database restart. Provider
failure diagnostics retain only typed stages/failures, without response prose,
request headers or credentials. No global economic ledger or spending authority
is changed. Owner review of the exact result checksum is still required before
private hypothesis retention or task completion.

### Exact first-evaluation authorization

No real request was authorized or performed by this preparation. First approve
**one local synthetic evaluation**, naming its creator UUID, owned agent UUID,
persisted task UUID/input SHA-256, `gpt-6-luna`, OpenAI project, unique budget UUID,
absolute expiration no more than ten minutes away, 2,000/256 caps and total cost
ceiling **378 microdollars ($0.000378)**. Approval includes sending the synthetic
notes/schema for metadata/count validation and one generation, no retries/tools.
A privately configured project-scoped credential is required; never put it in
chat, Git, command arguments, screenshots or evidence. No credential was read,
installed or rotated by these tests.

The prepared entry point additionally requires all of these explicit local
process settings; none are enabled by default or installed on Railway:

- `SUPA_SUPERVISED_REAL_MODEL=true` and `SUPA_ALLOW_OPENAI=true`.
- `OPENAI_MODEL=gpt-6-luna`, with `OPENAI_PROJECT_ID` equal to the plan's project.
- `SUPA_SUPERVISED_REAL_BUDGET_ID` equal to the plan's one-use budget UUID.
- `SUPA_SUPERVISED_REAL_TASK_ID` and `SUPA_SUPERVISED_REAL_OWNER_ID` equal to the
  approved persisted task and authenticated owner.

Keep all workers, autonomous execution, external tools and financial capabilities
disabled. Prepare the real plan through the existing authenticated task API;
the normal UI continues offering only mock reasoning. Review the displayed exact
plan, approve, Start and inspect; then manually request the inference step once.
An interrupted response means refresh/inspect, never repeat generation. Accept a
valid result explicitly or Stop it. Remove the one-task gate after evaluation.
Hosted integration, SQL application, credentials and deployment remain separately
controlled and have not occurred.

Prepared reasoning-upgrade SHA-256:
`3cd6e33c66173e53850c0b824d6fc2d4c0c49fa30c8cbf66dc2ac91e08f87df8`.
This edits only the not-yet-installed reasoning upgrade; the original supervised
SQL and all original migrations remain unchanged.

### Real-adapter preparation validation

Final local validation passed: 82 domain/provider/runtime unit tests, 17 loopback
wire fault scenarios within one integration test, nine session/adapter tests,
TypeScript and ESLint. The disposable PostgreSQL 18.3 suite passed Factory,
supervised tasks, real-plan cost reservations, same-budget rejection, unknown
usage, cancellation, owner review, unchanged financial tables, real database
restart and cleanup. Existing cross-owner, revoked-owner, expiry, atomic audit
failure and duplicate/concurrency cases remain enabled.

Wire faults include authorization expiry during the final dispatch guard,
unavailable/malformed/oversized counts, model mismatch,
unsupported findings, missing/over-limit usage, unapproved tool output, HTTP 429,
transport loss, timeout and revocation after token counting. The captured count
and generation contexts match exactly; a second invocation cannot send again.
No real-model credential, metadata, token-count or inference request was used.

Desktop/mobile browser journeys passed for both synthetic creators. Additional
private views displayed **simulated real-provider receipts**, including unknown
costs; these screenshots test rendering and are not evidence of a real AI call.
Evidence is under ignored `artifacts/supervised/real-model/`, with screenshots at
`artifacts/supervised/*-simulated-real-receipt.png`. Browser external requests were
zero. No new dependencies, production image builds or vulnerability scans were
performed. GitHub Actions and all live services remain untouched.


## Output-validation correction after the first authorized evaluation

The preserved first evaluation made one request and reported 936 input / 256
output tokens. It failed `model_response_rejected`, retained no candidate report
or private memory, and did not retry. The computed charge remains 222 microUSD
(rounded up from 221.6); the original reservation was 378 microUSD. This is
usage-based accounting, not an independently reconciled provider invoice.

The original parser discarded response status, incomplete reason, request and
response IDs, and the detailed validation rejection. Therefore its exact failure
cannot be reconstructed. Reaching the output ceiling makes truncation plausible,
not confirmed. A complete HTTP body does not imply a completed model response.
The historical evaluation artifacts and stopped database remain preserved.

The real supervised adapter now uses a compact, strict wire schema: status,
short assessment, one material citation (1-based line and verbatim quote), and
uncertainty. It normalizes to the unchanged `WorkProposal` contract; existing
citation, usage, owner-review and memory checks still apply. Tools, totals,
steps and memory references are empty because this approved scenario permits
none. The assessment remains provisional; exact quotations do not establish
causation. The schema and instructions aim to fit 256 output tokens; local size
checks do not guarantee a future model response will finish within that ceiling.
Full provider-side input counting remains mandatory before any future dispatch.

Operational evidence retains only allowlisted categories and bounded fields:
HTTP status, request/response/message IDs, verified model, response status,
truncation/content-filter/refusal categories and safe schema/evidence paths.
No raw prompt, output, refusal text, unknown JSON keys or transport exception
strings are retained. A missing/unsafe ID is omitted. Metadata/count request
IDs are cleared before generation so they cannot be mistaken for its ID.
OpenAI model diagnostics now survive the existing domain validation filter;
previously that filter accepted only Claude model names. Existing diagnostic
receipts deserialize with the new optional fields absent.

The handler rejects incomplete responses even if their partial JSON looks
valid; malformed JSON, extra fields, missing citations, unsupported quotations,
refusals and extra actions also fail closed. Known valid usage is settled even
when output is rejected; ambiguous transport retains unknown cost/reservation.
Rejected results cannot reach review, completion or private-memory retention.
The one-shot guard, exact owner approval, expiry, 2,000/256 token caps and
378-microUSD ceiling are unchanged. There is no automatic retry.

Reference: [OpenAI structured-output failure cases](https://developers.openai.com/api/docs/guides/structured-outputs).
No provider request was made while implementing this correction. A future
real-model evaluation requires separate explicit authorization and fresh exact
task/budget/expiry preflight; the consumed original authorization cannot be reused.

### Separate local psql loader issue

The extracted PostgreSQL 18.3 `psql` selected the system `libpq.so.5`, which lacks
`PQsendPipelineSync`. Its bundled `libpq.so.5.18` exports that symbol. Selecting
the bundled library for this process alone makes `psql --version` succeed:

```sh
LD_LIBRARY_PATH="$PWD/artifacts/ecosystem-pg18-binaries/root/usr/lib/x86_64-linux-gnu" \
  artifacts/ecosystem-pg18-binaries/root/usr/lib/postgresql/18/bin/psql --version
```

This version-only check does not connect to a database. No system library,
package, credential or service was changed. It is independent of model-output
validation and does not explain the rejected inference.

### Local correction validation

83 domain/provider/runtime unit tests passed. The actual adapter's loopback HTTP
suite passed 23 success/fault scenarios, including truncation, malformed JSON,
schema errors, missing/false citations, refusal, unsafe IDs, transport loss,
timeout and no-retry enforcement. Nine frontend session/authorization tests passed.
The disposable PostgreSQL 18.3 suite passed Factory, supervised reasoning,
desktop/mobile browser journeys and both restart-recovery checks. It verified
retained rejection diagnostics, priced rejected usage, blocked review of a failed
result, no retained result/memory, idempotent failure settlement and audit hashes.
The new fixture was stopped and removed; existing migrations/clusters were
unchanged. Secrets and one-file history audits passed. No dependencies, SQL,
UI, production images or live services changed.
