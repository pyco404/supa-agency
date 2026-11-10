# Railway autonomous X worker readiness

Latest local status, 2026-11-10: the runtime remediation at the end of this report
supersedes the earlier image security NO-GO. Both replacement targets passed the
complete scans and container tests. They are ready for a separately authorized,
private mock-only Railway rehearsal, not live publishing. Earlier assessments,
image identities and scan results below remain historical evidence.

Initial assessment, 2026-11-09 (Africa/Lagos). The container implementation below supersedes the packaging-pending assessment. Runtime baseline:
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

Railway runs long-lived containers, not the existing systemd unit. The initial
assessment identified the following three gaps. The implementation section below
records their resolution and supersedes these original bootstrap proposals:

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
- PostgreSQL listens only on 127.0.0.1 inside this service; the private
  SCRAM-authenticated runtime login can execute only the reviewed functions.
  The original separate-OS-user proposal is superseded by the trusted single-UID
  container described below. SQL restrictions do not isolate the ledger from an
  OS-level compromise of this container. Root-owned activation receipts remain
  protected, and no platform database credentials enter the worker.
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


## Container implementation (local validation)

`deployment/x-worker.Dockerfile` now builds the existing live binary plus a closed
mock binary. The default final target is **mock**; `--target live` selects a
separate image without mock initialization SQL or a mock executable. The mock
image has no live posting executable. `x-worker.Dockerfile.dockerignore` admits
only source/manifests/migrations and the explicit X-only scripts/SQL: no dotenv,
private receipts, keys, history, artifacts or frontend assets.

The image runs as UID/GID 10001, with tini forwarding signals to a Python
supervisor. PostgreSQL and dispatch descendants inherit a lifetime kernel lock
on the single volume. A private, fsynced `lease.json` retains holder UUID,
monotonically increasing generation and heartbeat deadline across boots. **The
deadline never authorizes stealing a held lock.** This is a single-volume lease,
not a distributed lock across copied databases. Freeze the laptop writer and
retain one authoritative database during the separately approved cutover.

The implementation deliberately uses one OS UID for the X-only container's
trusted processes (rather than the assessment's proposed two OS UIDs). SQL uses
a different, restricted runtime login with no role membership or raw-table DML;
no platform database or credentials enter this service. An OS-level compromise
of this container can access its X-only data/custody; SQL restrictions are not a
sandbox against the container operator. Do not run untrusted plugins, shell
commands supplied by users, or platform workloads here.

A volume at `/etc/supa-agent` must be explicitly prepared by its operator:
root-owned mode 0755 at the mount root, with `x-worker` owned by 10001:10001,
mode 0700. Live activation/control directories remain root-owned as required by
the existing receipt verifier. The publisher never chowns the mount, runs as
root, or requests `RAILWAY_RUN_UID=0`. The disposable Docker rehearsal performs a
separate root-only initialization of its newly created test volume; that is not
a root-running publisher or a change to a real volume. Railway volume ownership
preparation remains an owner-only prerequisite before the hosted rehearsal.

`deployment/x-worker.railway.toml` selects this Dockerfile, one replica,
`/healthz`, bounded failure restarts, no sleep, no overlap and a 60-second drain.
It requires the `/etc/supa-agent` volume mount before deployment can proceed.
No public domain/TCP proxy is needed. Resource ceilings and volume creation still
require separately approved Railway service setup; the config does not alter
existing services or attach the platform database.

### Startup and controls

- `SUPA_X_WORKER_MODE` defaults to `paused`; the explicit runtime values are
  `mock` for the mock image or `live` for the live image. A mismatched mode fails.
  A fresh service starts paused. A previously owner-resumed exact, unexpired grant
  can continue after safe recovery; changed, terminal or quarantined authority
  stays paused. Restart never creates or renews a grant.
- Mock initialization occurs only in a **new**, private directory and only in the
  mock image. It installs the reviewed five X-only SQL dependencies and synthetic
  records, a function-only login, and one paused mock grant. The expiry is retained
  in the private manifest; restarts never renew it. Any supplied X credential or
  DATABASE_URL causes mock startup/control to fail closed.
- The live image never initializes, restores, migrates or grants anything. It
  requires imported PG18 storage plus a private `ledger.json` with version 1,
  mode `live`, `import_verified: true`, and the exact imported `grant_ids` list
  (maximum 128). These are owner-recorded cutover assertions, not an independent
  cryptographic restore proof. The cutover comparisons above remain mandatory.
- Live `DATABASE_URL` must be the restricted `supa_x_post_runtime` login to
  `127.0.0.1:55483/supa_x_first_post`, without query/fragment host overrides.
  PostgreSQL retains imported SCRAM configuration and is forced to loopback.
- Either mount a private, worker-owned credentials JSON file at the existing fixed
  `/run/credentials/supa-x-autonomy.service/oauth1.json`, or provide all four
  existing X variables as service-only sealed variables. The latter are written
  once to that private ephemeral path; an existing mount is never overwritten.
  A restart accepts an identical private file after constant-time comparison,
  while mismatched values stop startup without replacing credentials.
  The four values are removed from child environments and never logged. Keep
  core dumps disabled. Never give live credentials to a source build or rehearsal.
- `SUPA_X_ACTIVATION_RECEIPT` identifies the existing root-owned receipt. Credential
  possession, health checks, and mode selection cannot authorize publishing.
  Root receipt, exact account/owner/expiry/budget and durable grant checks remain.
- On live boot, imported grants are recovered and paused using the new restricted
  `recover-paused GRANT_UUID` command. This works after expiry but can only reduce
  authority; it cannot load credentials, authorize, resume or dispatch. Terminal
  grants remain terminal. Owner-authorized new grants are added to the private
  restart inventory before enabling observation.
- Owner controls are through authenticated Railway administrative shell access:
  `python3 /opt/supa-x/entrypoint.py control read|authorize|resume|pause|stop|revoke`.
  They are not web endpoints. Resume still needs exact root-installed live
  authority. Restart continuation additionally requires the persisted active
  marker to match the exact receipt ID and SHA-256; uncertain recovery never
  resumes. An expired or changed receipt requires explicit fresh owner action.
- `/healthz` is read-only and reports sanitized state, PG readiness, lease generation
  and observation timestamp. It rejects stale/unready observations with HTTP 503;
  no authorization receipt, draft/source text or credentials enter its output.
- SIGTERM stops dispatch before stopping PostgreSQL. Failed child outcomes pause
  observation; no automatic dispatch retry. Partial attempts recover uncertain.

### Validation commands

Run from the repository, without sourcing dotenv:

```sh
CARGO_TARGET_DIR=target/work-review-validation CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p supa-runtime --bin supa-x-autonomy --bin supa-x-mock-worker
python3 scripts/test-x-worker-container.py
python3 scripts/test-x-hosted-worker.py
python3 scripts/validate-social-autonomy.py
docker build --target mock -f deployment/x-worker.Dockerfile -t supa-x-worker:mock-local .
python3 scripts/validate-x-worker-docker.py --report-dir artifacts/x-worker-remediation/final
```

The Docker rehearsal creates/removes only one new named test volume/container,
uses `--network none`, dropped capabilities, no-new-privileges and a read-only
root filesystem. It checks default pause, actual mock posting, duplicate-worker
rejection, uncertainty, real container restart, history/audit integrity and clean
SIGTERM. It never receives real credentials or the actual ledger. The full
PostgreSQL suite separately checks caps, permission/identity boundaries, fault
handling and restart recovery. The new process tests also exercise expiry,
rolling caps and inherited-lock exclusion after parent loss.

Local container testing uses a task-isolated rootless Docker daemon, not a
system Docker installation or Railway service. Host AppArmor/cgroup delegation
is unavailable in that environment; no production profile is changed. Local
resource measurements cannot attest Railway cgroup ceilings. Hosted mock testing
must verify those ceilings before live activation. Reports are private under
`artifacts/x-worker-container/`; no GitHub Actions artifact storage is used.

Status and final image/security results must be reported from the completed
validation output. An incomplete build/scan/rehearsal is **not** a successful gate.
Neither a live image build nor these instructions authorize image publication,
Railway deployment, ledger migration or X activation.

### Completed local validation — 2026-11-10

Both complete Docker targets built successfully, using PostgreSQL 18.6
(`18.6-1.pgdg12+2`) and non-root UID/GID 10001. Standard 16 MiB WAL segments
are covered by the disposable PostgreSQL test; `min_wal_size=32MB` avoids an
incompatible startup setting on a normally initialized imported ledger.

| Local image | Configuration ID | Uncompressed bytes |
| --- | --- | ---: |
| Mock | `sha256:2fd82ca9db97eb366b5eea3f3b0c2056a27f040cef4e65792d4c38812947d098` | 483,772,052 |
| Live | `sha256:220b28c5ae9ec4b982a855863ed1ad71ec4256183825077a35202ceb0d09dbd9` | 485,785,233 |

These are local configuration IDs, **not published registry manifest digests**.

- Nine process/storage/PG18 tests passed, plus three existing hosted-worker checks.
- All six existing disposable PG18 journey, boundary and restart checks passed.
- Eight actual Docker rehearsal checks passed with networking disabled. The
  container booted paused, rejected a duplicate publisher, completed one mock
  dispatch, continued an unchanged authorized grant across a clean restart without
  reposting, preserved uncertainty/history across another restart, rejected resume
  while quarantined, and exited cleanly on SIGTERM. Its temporary volume/container
  were removed. No real credential or original posting ledger was used.
- The live image separately failed closed without an imported ledger. Its binary
  loads, and neither mock executable nor mock SQL fixtures are present.
- Observed aggregate RSS was 118,332 KiB (about 116 MiB). This snapshot includes
  shared pages counted in multiple processes; it is not a peak/load guarantee.
  The proposed 512 MiB/0.25 vCPU Railway ceiling remains untested and unapplied.
- Rust formatting, Python compilation and TOML contract checks passed. Deployment
  fields were reviewed against Railway's official schema; automated schema
  download from this workstation returned HTTP 403, so a complete automated
  remote-schema validation is not claimed.

**Security gate: NO-GO for hosted deployment.** Trivy 0.76.1 scanned both complete
images using its freshly downloaded database, without an ignore file or severity
filter. Each image reports 4 CRITICAL, 87 HIGH, 245 MEDIUM, 192 LOW and 2 UNKNOWN
findings, and zero embedded-secret findings. These are scanner classifications,
not proof that every finding is exploitable through this worker. None is
suppressed or accepted by this preparation.

Of the 91 CRITICAL/HIGH records, 65 are Debian package records and 26 are from the
inherited `/usr/local/bin/gosu` Go runtime. The worker does not use gosu, but the
file remains in this scanned image. Critical records include `CVE-2025-68121`
(gosu's Go runtime), `CVE-2025-7458` (SQLite), `CVE-2026-6653` (libxml2), and
`CVE-2023-45853` (zlib). The Debian records contain no fixed package version in
this scanner report; that is not permission to ignore them. A separate explicit
Cargo.lock scan identified 274 packages and zero reported vulnerabilities.

Private local evidence is under `artifacts/x-worker-container/`: image identities,
complete scans, security summary, rehearsal/cleanup results, offline live smoke
checks and existing PG18 test references. Repository secret checks passed; staged
new files are scanned again by mandatory commit hooks.

Before a hosted rehearsal, remediate/review the runtime base and unused inherited
helper, then rebuild and rerun the same container/security checks. Do not publish
these images or bypass the findings to proceed. Only after that gate clears is
one separately authorized private, credential-free mock Railway service/volume
rehearsal appropriate. No service setup, credential transfer, ledger migration,
account lookup, paid inference or X posting occurred during this validation.

### Runtime security remediation — 2026-11-10

The correction changes only the Docker recipe, the container validation harness,
and this report. Worker, scheduler, ledger SQL, OAuth and supervision source are
unchanged. Both replacement Rust executables are **byte-identical** to those in
the previous images: mock SHA-256 `c6fc05bf556a81f54a76e8270c03051952a74210acacec0070aeb3789d0c664d`,
live SHA-256 `74a6942a505d9face11b354abf8b5d540401898cf2e492abfbbe7a2a3019ba3e`.

All 91 earlier CRITICAL/HIGH records were in final runtime layers; none came from
the excluded Rust builder. Exact package versions, CVEs, fix status and full
layer IDs are preserved in `artifacts/x-worker-remediation/baseline-attribution.json`.

| Original layer (DiffID prefix) | Origin | CRITICAL/HIGH records |
| --- | --- | ---: |
| `1d7b9dd8778e` | Debian Bookworm root filesystem | 35 |
| `9b536e93367c` | Worker Python/tini installation | 12 |
| `aa38d5ea85a2` | Upstream gnupg/less installation | 8 |
| `51747bf5584d` | Upstream PostgreSQL installation | 10 |
| `7ac954ae7a14` | Unused gosu helper and embedded Go 1.24.6 runtime | 26 |

Installed vulnerable libraries are distinguished from proven exploitation: the
old worker never invoked gosu, and no exploitability claim is made for every
Debian library finding. In particular, the scan marked the old zlib record
`will_not_fix`. No reachability exception, suppression, or severity downgrade was
used to pass the replacement image. The affected old packages/layers are absent.

The new runtime uses the official [Wolfi base](https://www.chainguard.dev/docs/open-source/wolfi),
pinned to `sha256:d13acb327aa77acf2752acea8c08456511a5b8142455cc2fe0f966b960bf64c6`.
It retains glibc compatibility with the existing Rust executables. Signed APK
installation pins PostgreSQL server/client `18.6-r7`, Python runtime
`3.14.10_git20261104-r0`, tini `0.19.0-r26`, CA bundle `20261020-r0` and BusyBox
`1.38.0-r3`; installed transitive versions/checksums remain in the scanner inventory.
The normal package resolver is used without untrusted-package or broken-world
flags. Both the unnecessary `wolfi-base` metapackage and `apk-tools` are removed
after installation; keeping the metapackage alone would retain the package manager.
The installed-package database stays available to scanners.

There is no gosu, Go helper, Bash, package manager, Rust toolchain, compiler or git
in the final runtime. BusyBox `sh` remains because PostgreSQL initdb/pg_ctl use it.
Path aliases preserve `/usr/lib/postgresql/18/bin` and existing client commands;
no supervisor or ledger rewrite is needed. `STOPSIGNAL SIGTERM` is explicit rather
than inherited. UID/GID 10001, default pause, read-only health reporting, credential
paths, volume lease and authority checks remain intact.

Representative replaced packages are SQLite `3.54.1-r0`, libxml2 `2.15.0-r0`
(plus the LLVM-required compatibility package `2.13.9-r2`) and zlib
`1.3.2.1_rc20260921-r2`. Their exact inventories and scanner results are retained;
there are no remaining reported vulnerabilities requiring fixes in either image.

| Final local image | Configuration ID | Uncompressed bytes |
| --- | --- | ---: |
| Mock | `sha256:38e296cc6e9b4689aaeec6a2905edba4038c86917fcc558aa0406d866b9ce143` | 349,829,037 |
| Live | `sha256:2bd913edc55b1e6447a2c0d08788a89cc78173e55265cff8e8f9250bcc0b276a` | 351,842,218 |

These linux/amd64 images are approximately 28% smaller than the originals.
Configuration IDs are local identities, not published registry manifest digests.

- Both complete targets rebuilt successfully. Trivy 0.76.1 with a freshly
  downloaded database inventoried 60 runtime packages per image and reported
  **0 CRITICAL, 0 HIGH, 0 MEDIUM, 0 LOW, 0 UNKNOWN and 0 embedded secrets**.
  Scans used `/dev/null` as the ignore file, all severities, and no ignore-unfixed
  option. Report image IDs match the exact built/tested configurations.
- A separate current Cargo.lock scan inventoried 274 packages and found zero
  vulnerabilities. The Debian Rust compiler stage remains build-only and is not
  represented as a vulnerability-free runtime component.
- All nine worker tests and three existing hosted-worker checks passed. The six
  existing PostgreSQL regression tests passed locally and again inside the new
  PostgreSQL 18.6 image, including both restart tests and permission boundaries.
  Host test executables were mode 0700; verified byte-identical executable copies
  were mounted read-only for the non-root test process, leaving originals untouched.
- All nine Docker checks passed: non-root/default pause, minimal runtime and TLS,
  one-publisher exclusion, bounded mock dispatch, duplicate/spacing enforcement,
  restart continuation without repost, uncertain-outcome quarantine, preserved
  audit/history and clean SIGTERM shutdown. Runtime TLS checks verified glibc
  2.44, Python's OpenSSL 4.0.3, 118 trusted CAs, required certificate verification
  and hostname checking. The X transport's Rust TLS code/binary is unchanged;
  no live X handshake or account request was performed.
- Observed aggregate RSS was 123,320 KiB (about 120 MiB). This is a snapshot with
  shared pages counted more than once, not a peak/cgroup guarantee. Railway
  resource ceilings remain to be verified in the approved hosted rehearsal.
- Live offline smoke checks passed: a missing imported ledger fails closed and
  no mock dispatcher or fixture is present in the live target.

Reports are private under `artifacts/x-worker-remediation/`, separately from the
preserved baseline evidence. `security-summary.json`, `images.json`, complete
scans, installed-package inventories, scanner provenance, final rehearsal and
PostgreSQL logs record the result. Initial failed minimality/fixture checks are
retained as well; no mandatory check was removed or treated as a success.

**GO for one separately authorized private mock-only rehearsal.** No push,
publication, Railway change, real ledger access, credential transfer, paid request
or live post occurred. The existing reviewed restore/schema/receipt comparisons
remain mandatory before any future live ledger cutover; this fresh synthetic
test does not authorize copying a real cluster or activating publishing.


## Worker release isolation — November 10, 2026

The existing staging `supa-web` service is connected to application `main`.
Its watch paths were previously empty and no config-as-code manifest was active.
Only its watch-path setting was updated; no deploy was requested. Readback
confirmed the current frontend, API and PostgreSQL deployment IDs and sources
remained unchanged. API and PostgreSQL use immutable images and have no GitHub
repository triggers.

Frontend watch paths now cover:

- `/web/**` (including dependencies, lockfile, Next configuration and shared web modules)
- `/reference-web/**` (including page artwork, navigation media and shared scripts)
- `/deployment/frontend-railway.Dockerfile`
- `/deployment/frontend-railway.Dockerfile.dockerignore`
- `/deployment/frontend.railway.toml`
- `/scripts/scan-secrets.py`
- `/scripts/test-public-landing.mjs`
- `/.dockerignore`, `/railway.toml`, `/railway.json` (root build/config fallbacks)

The frontend Docker recipe copies no Rust crates or Cargo manifests. Its 33
checked source/build inputs match the policy; the worker release paths do not.
Railway documents watch paths as gitignore-style patterns. This preparation
checks those semantics and the server-side readback; it does not claim an
unperformed GitHub webhook test. If config-as-code is connected later, maintain
these exclusions and dependency coverage in that manifest too.

The isolated source is based on GitHub `main` at
`245fdd71001c68504f854cead0916c3d7cb38a95`. It transplants 130 existing one-file
commits for the X libraries, SQL, worker/container implementation, regressions
and shared literal-configuration dependencies. This new audit note is a separate
one-file commit. The private `.env`, credentials, real ledger, newer reasoning,
Factory, trading, API and website changes are excluded. Original local `main`
and all its unpublished history remain preserved. The named release reference
is `release/x-worker-mock-20261110`; no branch is pushed during preparation.

The isolated runtime source passed locked/offline compilation, 66 Rust tests
(23 separately provisioned provider/custody tests remain ignored), six disposable
PostgreSQL 18.3 integration/restart checks, nine worker tests and three hosted
contract tests. The retained validated PostgreSQL 18.6 mock container separately
passed nine fresh Docker checks with networking disabled, including non-root,
paused startup, single publisher, durable history and uncertainty quarantine.
Fresh all-severity Trivy scans of both retained images found zero vulnerabilities
and zero secrets. Those retained images belong to the previously validated
source, not a successful build of this new isolated release tree.

Fresh isolated-tree Docker validation remains **blocked by local resources**.
An initial temporary rootless build lacked default container networking; a
build-only network selection resolved DNS. The subsequent build was stopped at
the owner's instruction, not accepted as passing. Its daemon, stopped containers,
images, logs and files are retained. No proposed cleanup is authorized or done.
No real X credentials or posting records were transferred, and no paid API calls,
GitHub builds, publication or Railway worker services were created.

Prefer one manual artifact-free `ubuntu-24.04` validation job after its source
availability and authorization are resolved. Build/test both targets on the same
runner, run full vulnerability/secrets gates and mock-only PostgreSQL/container
checks, and record sanitized summaries and hashes. Do not upload image archives,
require registry write permission, publish, deploy, or supply X credentials.
GitHub source publication and a runner launch require separate approval while
pushes remain prohibited pending validation. Existing frontend-native validation
is a reusable runner pattern, not an existing X-worker validation workflow.

**NO-GO for a new isolated-image rehearsal until its complete Docker validation
passes.** Frontend watch isolation is configured; it does not waive any worker
security, resource, paused-mode or storage-ownership prerequisite.
