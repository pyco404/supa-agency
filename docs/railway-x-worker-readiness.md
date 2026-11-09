# Railway autonomous X worker readiness

Read-only preparation, 2026-11-09 (Africa/Lagos). Runtime baseline:
`dff7e2acf8a14931322562e2f74b5ca5a8333528`. No deployment, credential transfer,
ledger export/restore, live X request or activation is authorized by this document.

## Decision

Use one new private, volume-backed `supa-x-worker` service in the existing
Supa Agent project, containing the X-only worker and its own PostgreSQL 18
process. This is a proposed packaging choice, not a working Railway image yet.
Do not attach it to `supa-postgres`, `supa-api` or their database credentials.
A worker plus a second dedicated database service is a valid alternative, but
requires two services and a separately reviewed private-network database adapter:
`connect_first_post_local` currently rejects non-loopback hosts deliberately.

One combined service preserves the existing loopback-only database checks and
uses one volume, at the cost of supervising two processes and accepting brief
restart downtime. For this low-throughput, bounded trial it is the smaller change.
It does not provide database high availability or independent disaster recovery.

Railway runs long-lived containers, not the existing systemd unit. Three pieces
remain to implement and test before any hosted rehearsal:

1. A small image with the existing Rust binary, PostgreSQL 18 and a proper PID 1
   supervisor. Forward SIGTERM, stop the worker before PostgreSQL, reap children,
   and fail closed on database loss. No automatic schema installation on startup.
2. A root-only bootstrap that materializes the four existing OAuth1 values from
   service-scoped sealed variables into the fixed private credential file expected
   by the worker. It installs the separately owner-approved non-secret receipt
   under the existing protected `/etc/supa-agent/x-autonomous-publishing` path.
   Preserve UID, permissions, ancestor, hash and revocation checks; never skip them
   to make the container run. Drop privileges before running Rust/PostgreSQL and
   remove X credentials from child environments. No secret build arguments/layers,
   dotenv copies, shell tracing, environment dumps or credentials in health output.
3. An explicit mock entrypoint/rehearsal image reusing the existing mock transport
   and PostgreSQL fixtures. No real credentials, live receipt or live dispatch
   entrypoint in the rehearsal. This must exercise the container supervisor and
   volume path, not just rerun Rust tests outside the container.

The original systemd path remains usable on a conventional Linux host; its
`LoadCredentialEncrypted` mechanism is not a Railway feature. Service-scoped
sealed variables are the proposed Railway custody replacement, not an extra
keyring enrollment. They are also available to builds, so use an already-built,
reviewed image and never build with the live credentials installed.

## Read-only account verification

Fresh explicit-project CLI status succeeded; the repository itself is unlinked,
so no local `railway link` or service configuration was changed.

- Project: `99648d2a-69a1-4d8c-98d0-7f2a26543c9c`, Supa Agent.
- Environment: `ff0b2092-e42b-4a89-ae25-441ab95d92d8`, staging.
- Existing `supa-api`, `supa-web`, `supa-postgres` deployments each report SUCCESS
  with a RUNNING instance. This is deployment status, not a new SQL/HTTP health test.
- Existing account reports Pro; current usage approximately $24.50,
  projected period bill $34.27, existing workspace hard limit $60 and soft limit
  $20. Billing period ends 2026-11-18 17:34:07 WAT. No limits were changed.
- No X worker exists in the observed service inventory. No existing resource
  ceiling or configuration needs to change for this proposal.

## Proposed service settings

- One region, one replica, one persistent volume initially 1 GB (measure the real
  ledger and restore/WAL working space before approving size).
- Serverless/app sleep OFF; no cron, autoscaling, public domain or public TCP proxy.
  Do not rely on incidental database traffic to keep the scheduler awake.
- PostgreSQL listens only on 127.0.0.1 inside this service; private SCRAM-authenticated
  runtime login can execute only the reviewed functions. Separate PostgreSQL OS
  user and non-root worker UID; the worker cannot read database storage or root
  control files. Database administrative credentials never reach the worker.
- Initial combined service ceiling: 512 MiB RAM, 0.25 vCPU; confirm under rehearsal
  load. Candidate PG tuning: 64 MiB shared buffers, 10 connections, 2 MiB work_mem.
  These are proposals, not measured capacity or applied settings.
- Persistent volume contains the dedicated database and restrictive control state;
  runtime credentials remain ephemeral. Refuse a missing/mismatched imported
  ledger; never initialize an empty live ledger automatically.
- Disable automatic deployment during the trial. No overlap and a 60-second
  shutdown grace. Railway volume exclusivity plus a lifetime local process lock
  must prevent two supervisors on the same volume. Database account locks and
  one-use claims remain the final dispatch fence.
- Bounded crash restart/backoff; persistent uncertain attempts quarantine instead
  of replaying. No startup authorize/resume. An already authorized, unexpired
  active grant may continue only after safe recovery; paused/revoked grants stay so.
- An expired 24-hour grant remains idle/paused without a restart storm. Always-on
  hosting does not mean indefinite publishing authority or automatic renewal.
- Health/heartbeat reports only readiness, last observation and paused/active/error
  category. It must distinguish a healthy paused worker from failed PostgreSQL or
  a dead worker; no account tokens, draft text, source data or database URL in logs.
  Do not add unauthenticated control routes for health checking.

Railway volumes prohibit replicas and simultaneous deployments mounting the same
volume. This is useful but does not fence a second copy of the ledger elsewhere.
The laptop publisher must be stopped before transferring publishing authority.

## Cost estimate

Current public rates: RAM $10/GB-month, CPU $20/vCPU-month, volume $0.15/GB-month,
egress $0.05/GB. Existing Pro subscription is shared; no additional subscription
fee per service. Actual resource usage, not the ceiling alone, determines charges.

| Scenario | RAM | Average CPU | 1 GB volume | Approximate monthly resources |
| --- | --- | --- | --- | --- |
| Light measured-use target | 0.25 GB | 0.01 vCPU | $0.15 | $2.85 + egress/backups |
| More conservative use | 0.50 GB | 0.02 vCPU | $0.15 | $5.55 + egress/backups |
| At proposed CPU/RAM ceiling continuously | 0.50 GB | 0.25 vCPU | $0.15 | $10.15 + egress/backups |

These are arithmetic estimates, not measured worker utilization or a new spending
approval. Confirm billing units, actual volume allocation and account headroom at
activation. Existing $60 workspace hard limit can stop all workloads; leave it
unchanged. Estimated one-day host resources are roughly $0.10-$0.34 using a
30-day month. Six X attempts separately reserve up to $0.15 at the existing pinned
$0.010 identity-read + $0.015 plain-text-create rates; recheck tariff and credits
before activation. Failed attempts are conservatively counted and reserved.

## Ledger cutover (requires separate approval)

1. Pause/stop the laptop scheduler and disable every other Supa publishing writer.
   Resolve or preserve all unfinished attempts as quarantined, never erase them.
   Keep a clean shutdown/cutover receipt proving which database is authoritative.
2. Export one transactionally consistent full X-only ledger with roles/grant
   definitions under authorized private handling; checksum and encrypt it before
   transfer. Do not export or change the Railway platform database.
3. Restore to isolated target PostgreSQL 18; preserve the real first-post receipt,
   drafts, attempt UUIDs, account ID, reservations, grants, audit-chain hashes and
   timestamps. Compare all schema/table/sequence inventories, five reviewed SQL
   hashes, data counts/content hashes, function owners and restricted grants.
4. Apply only the separately approved `social-autonomy-host-grants.sql` if absent.
   Verify first-post and autonomous runtime role checks and zero raw-table DML.
   No new platform owner, registration, database administrator or spending role.
5. Confirm restart persistence while both publishers are paused. Retain the source
   backup privately. Record target as sole authority before issuing a host-bound
   receipt. Do not point the laptop writer at its now-stale local database.
6. Roll back code with the same current ledger, not an older database snapshot.
   Returning authority to the laptop requires a new stop/fence and reverse transfer
   of the latest ledger. Never resume a stale backup after an uncertain POST.

No export, transfer, role change or database inspection was performed for this plan.

## Mock-only hosted rehearsal (prepared, not executed)

After local image/bootstrap validation and separate deployment approval, create
only the proposed private service/volume using synthetic records and dummy
credentials. Do not install the real account credentials, history or activation.
Use the existing mock provider, strict mock-mode policy and clock-controlled test
fixtures; test time controls must not be compiled/exposed in the live entrypoint.

| Check | Required evidence |
| --- | --- |
| Boot and default pause | PG readiness succeeds; no network dispatch before explicit mock grant/resume |
| Mock publish | Exact mock account, genuine synthetic evidence, one reservation/claim/receipt and audit chain |
| Limits | Sixth-attempt ceiling, rejected seventh, rolling-window boundary, >=4h spacing, total budget |
| Duplicate/competition | Two worker starts/duplicate task IDs produce at most one mock POST |
| Crash | Kill during claimed dispatch; restart preserves reservation, quarantines and does not resend |
| Restart/redeploy | Same volume/history/grant IDs; no implicit resume of a paused grant or reset caps |
| Failure | Account mismatch, 401/403, 402, 429 and ambiguous transport pause/quarantine correctly |
| Authority | Missing/expired/revoked/mismatched receipt and unsafe credential ownership fail closed |
| Emergency | Pause/revoke/stop block new dispatch; SIGTERM reaches children; PG shuts down cleanly |
| Resources | Peak RSS/CPU/WAL/disk remain within approved ceiling; logs contain no credentials/source text |

Keep only sanitized test counts, hashes, mock receipts, elapsed times and resource
measurements. Preserve the real source ledger untouched. Rehearsal success is not
live account verification or proof that X will accept a post.

## One controlled 24-hour live activation

Only after the rehearsal, image scan and private restore/restart checks pass:

1. Owner approves the exact image, service, bounded host spend and private cutover.
2. Owner privately installs the existing four OAuth1 credentials as worker-only
   sealed variables; confirm Read/Write permissions and available X API credits.
3. Install one fresh owner/account/UID-bound receipt for <=24h, <=6 attempts,
   interval >=14400 seconds (existing SQL adds 8 seconds), budget <=150000 microUSD.
   Keep the same grant ID across restarts. The exact verified numeric account ID
   comes from the private receipt, not a username guess.
4. Approve one batch of genuine, public cycle/goal events. No per-post approval.
   Arbitrary public-memory summaries and a continuous platform-event feed are not
   yet connected; private memories remain excluded. Abstain when sources run out.
5. Perform offline custody/role/ledger checks, install the grant paused, explicitly
   resume once under the same activation and monitor durable receipts.
6. Before every POST, the existing signed adapter verifies account identity and
   durable authority. No retries after ambiguous outcomes. Stop at expiry/cap,
   revoke/pause on demand, and review ledger/audit/history at the end of the trial.

Status: Railway feasible; packaging/bootstrap and hosted rehearsal remain pending.
No live readiness claim, service deployment, activation or paid X calls.

## Official references (checked 2026-11-09)

- https://docs.railway.com/pricing/plans
- https://docs.railway.com/pricing/cost-control
- https://docs.railway.com/deployments/serverless
- https://docs.railway.com/volumes/reference
- https://docs.railway.com/variables
- https://docs.railway.com/deployments/deployment-teardown
- https://docs.x.com/x-api/getting-started/pricing
