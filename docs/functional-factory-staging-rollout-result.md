# Functional Factory synthetic staging rollout — 2026-11-08

GO for the two authorized synthetic creators only. Real registration and real-user
onboarding remain prohibited. The successful Railway logical restore does not
replace the required independent encrypted off-platform recovery.

## Target and completed changes

Project `99648d2a-69a1-4d8c-98d0-7f2a26543c9c`, staging environment
`ff0b2092-e42b-4a89-ae25-441ab95d92d8`. PostgreSQL service
`77352ca9-435e-4870-b092-58365310c0f3` remains on deployment
`db546a74-4038-4536-b450-d93e78362808`, PostgreSQL 18.6. It was not
restarted or replaced. All three services are healthy.

Applied once, in the approved order; SHA-256:

| SQL stage | Checksum |
| --- | --- |
| ecosystem-social | `e50180e1cdb0d103d2d8a85d84d16b8debfe8328456f4b6a8ebb3222068e2b3c` |
| agent-factory | `902ed919f74b9b56fb0a7cf3dc62b063cc3db404efbeb4a656da40cfdae5bd56` |
| agent-avatars | `94610db26d3b592f16638a0c0d8c312737f13d0a88d179acb7e5611b0496b3cd` |
| reviewed restricted grants | `3e6b1176f3927839a3bfbaada4b91223acb8ce3c24f5233be7f7078783ade519` |

There are now 64 tables and six sequences. The original 13 migration records and
checksums are unchanged. Feature schemas/functions are migration-owned; the
runtime has exactly five approved function grants and two feature-schema USAGE
grants, no direct feature-table privileges, no owned objects or elevated role.
PUBLIC function grants remain revoked. Nine unauthorized write/DDL/escalation
operations were denied. Existing runtime table privileges were preserved.

The grant verification initially encountered multiline PostgreSQL JSON. The
stream parser was corrected locally and installed state verified without repeating
any installer or grant statement. No schema was reversed.

## Images and deployment identities

One API replacement and one frontend replacement completed; no build, publication,
credential rotation, manual registry-token request or deployment retry occurred.
Railway's encrypted existing credentials pulled the exact approved private images.
Both replacements required two consecutive complete health/security passes.

| Service | Deployment ID | Running immutable image |
| --- | --- | --- |
| supa-api | `0499b7f4-d1eb-4bfe-bf25-22384db1f996` | `ghcr.io/pyco404/supa-agency-api-infra-3b5d53616ab8@sha256:7624248d776414548d80fe0834c9ee608b6167765dcf25463f6e4197716f4cf9` |
| supa-web | `6e5693fc-f16f-415c-98ca-e6a598aecf66` | `ghcr.io/pyco404/supa-agency-frontend-infra-23137dd454b7@sha256:dafa15f49d6e4eb88e8c6e69f8d1ed3870ce8b39fcc85136f641fd6757aa9bcb` |

Verified source compatibility: API application
`3d9188345b290a4b1a28e48208b29307802b2bd9`, frontend application
`4dcab96e491a2ef686531dcd7efdecfcc451d397`; the relevant application contracts
and SQL match. Prior independent registry privacy, repository-association and
complete configuration/layer integrity evidence was rechecked.

Approved retained scans remain unsuppressed: API 0 CRITICAL, 0 HIGH, 34 MEDIUM,
8 LOW; frontend 0 CRITICAL, 0 HIGH, 1 MEDIUM (`CVE-2026-4738`). Both retained
secret scans found zero secrets. These are the approved image scans, not claims
of new scans during deployment. The previous two HIGH zlib advisories were absent
from the approved API scan.

## Synthetic identity scope and demonstrated workflow

Exactly two application identities, scoped as creators of their own agents:

| Creator principal | Created private agent |
| --- | --- |
| `0eafaab1-4fd4-4c45-a704-b589530bb5ce` | `65424a54-e711-4a39-8d93-9c2f11b3dacf` |
| `75ca3b86-64c6-4abc-9481-5859549f6f96` | `d547f1e9-728c-4727-aa52-7c9d0ed262dc` |

Both credentials expire **2026-11-09 02:41:18 UTC**; web sessions last 15 minutes.
No PostgreSQL login/role, platform-owner identity or registration facility was
created. Existing operator/viewer credentials and deadlines remain unchanged.
New credential material was handled privately; it is absent from this report.

Fresh desktop (1440×1000) and mobile (390×844) Chrome contexts completed:
Sign In → Factory → configuration → review → Create Agent → My Agents → private
detail. Name, description, purpose, personality, goals and requested permissions
were persisted. Desktop used an optional cropped/normalized private upload;
mobile used the deterministic default without an upload or payment. Both avatars
decoded successfully. Configuration views, refresh and responsive layout passed.

Each identity sees exactly its one private paused agent. Both have zero spending
authority, no enabled capabilities, disabled autonomy/execution, pending purpose
qualification and no token mint. Start/Pause/Resume/Stop and token launch remain
unavailable. Requested permissions do not enable execution.

## Security, persistence and recovery acceptance

- Other-owner detail reads return 404; other-owner avatar metadata/image reads
  return 403. Anonymous private requests return 401. No private image was exposed.
- Duplicate-key replay returns the original agent; a changed payload with the
  same key returns 409. Missing private consent returns 400. Exactly two agents
  were created; testing did not repeat either creation.
- A rollback-only transaction revoked one new synthetic ACL row. The owner list
  became empty and duplicate replay was denied with SQLSTATE 42501. ROLLBACK
  restored the original owner grant; no persistent ACL changes resulted.
- Both public consents are stored nonpublic/revoked with empty event categories.
  Private profiles return 404 publicly and the public directory is empty. No
  public-consent grant was introduced. Generic management mutations of Factory
  agents remain frozen; public-consent editing was not enabled.
- The reviewed API-only restart completed once on the same deployment/image.
  Persisted agent/configuration hashes and ownership matched before/after.
  Both creators signed in again and loaded their own agents after restart;
  cross-owner denial and sign-out invalidation passed. PostgreSQL was not restarted.
- Nine public sections passed desktop/mobile browsing; anonymous browsing made
  zero private API calls or writes and exposed no synthetic agents. Protected
  management, wallet, operator and API routes remained authenticated. The
  protected-route observation suite passed, with one bounded read-only retry
  after a transient staging request timeout. No deployment was retried.
- Existing operator/viewer accounts still access protected staging and API reads.
  The legacy authenticated `/operator` → `/agents` 302 matches reviewed code.
  Their Factory access is denied with 403. Cross-origin requests return 403;
  forged credentials return 401. HTTPS remains valid and www redirects with 308.
- Runtime PostgreSQL connections use TLS 1.3. The original synthetic fixture hash,
  all original migration checksums and protected accounting/unrelated table counts
  are unchanged. Only the expected agent, owner ACL, audit, activity and lifecycle
  rows were added alongside the additive feature records.

The initial live test incorrectly expected 404 for cross-owner avatar metadata;
the reviewed SQL intentionally returns 403. A status-only read confirmed denial.
Only this harness assertion was corrected; the remaining acceptance continued
without recreating agents or changing application security.

## Configuration, resources and boundaries

Only the approved synthetic account bindings and
`SUPA_SYNTHETIC_FACTORY_ENABLED=true` / `SUPA_AGENT_AVATARS_ENABLED=true`
were added to API/web configuration. Registry credentials, domains, service
origins and unrelated configuration were preserved.

`SUPA_SITE_ORIGIN=https://supa-web-staging.up.railway.app` and
`SUPA_PUBLIC_LANDING_ENABLED=true` remain unchanged. The private API URL remains
`http://supa-api.railway.internal:8080`; it has no public domain. Anonymous Railway
staging access remains authenticated. No DNS changes occurred.

Resource limits are unchanged: API 0.25 vCPU / 256 MiB; frontend 0.5 vCPU /
512 MiB; PostgreSQL 0.5 vCPU / 512 MiB, each one replica. No worker service or
recovery resource was created. Railway usage at final review: **$22.4078 / $60**,
not over limit. The change in workspace usage includes normal existing-service
usage and is not an isolated rollout-cost measurement.

Native Railway samples for 02:41:47–02:56:47 UTC showed API memory peak
0.012279808 GB and frontend peak 0.086433792 GB, below their limits; CPU_USAGE
maxima were 0.0012877 and 0.0374381 against CPU_LIMIT 0.25 and 0.5 respectively.
A separate frontend cgroup sample showed 85.75 MiB, zero OOM/OOM-kill events and
0.0017 CPU seconds over two seconds. The distroless API has no shell, so native
Railway metrics were used instead of changing its image or installing tools.

Provider remains offline; workers, scheduler, paid inference, controlled work,
wallet signing, trading, token launches and X publishing remain disabled. No
financial authority or real registration was enabled. No recovery was needed.

## Evidence

Sanitized local receipts and desktop/mobile screenshots are retained in
`artifacts/functional-factory/staging-rollout-20261108/` (ignored, not pushed).
Key receipts: database-stages, service-stages, live-browser, remaining-acceptance,
revocation-transaction, api-persistence-restart, database-before/after-restart,
post-restart-browser, existing-operator-viewer-staging, final-control and
native-metrics. Original recovery and image security evidence remains preserved.
The rollout's private RAM credential copy and disposable local browser fixture
were removed after acceptance; neither is retained with the evidence.

This report is local documentation only. No new Git push is part of the rollout.
