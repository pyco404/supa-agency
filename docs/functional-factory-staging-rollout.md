# Functional Factory — synthetic-only staging rollout preparation

Reviewed November 8, 2026, at 01:56–01:58 UTC (02:56–02:58 WAT).
Preparation and read-only verification only: no live DDL, identities, grants,
configuration changes, publication or deployments were performed.

The verified isolated Railway logical recovery clears the recovery gate for this
synthetic-only pilot. It supersedes the earlier recovery blocker for synthetic
staging; independent encrypted off-platform recovery remains mandatory before
real users or sensitive production records. This document does not authorize
rollout execution.

## Current target and health

Project `99648d2a-69a1-4d8c-98d0-7f2a26543c9c` (Supa Agent), staging environment
`ff0b2092-e42b-4a89-ae25-441ab95d92d8`. Protected primary excluded.

| Service | Current successful deployment | Resource ceiling, one replica |
| --- | --- | --- |
| supa-postgres | `db546a74-4038-4536-b450-d93e78362808` | 0.5 vCPU / 512 MiB |
| supa-api | `2238b7df-11b2-48ed-a1f0-7cab9796e1a7` | 0.25 vCPU / 256 MiB |
| supa-web | `51630c72-80ac-4a80-abff-f10fd8cdbbd6` | 0.5 vCPU / 512 MiB |

Service IDs, respectively: `77352ca9-435e-4870-b092-58365310c0f3`,
`9996e351-d7a5-482e-9f14-5f863eb9b45f`,
`955bb925-6343-433e-b9dd-f7cbdd89114a`.

Fresh SQL confirms supa_staging, cluster `7704432314739101731`, PostgreSQL 18.6,
verify-full TLS 1.3, all 13 original migration checksums, no feature schemas,
no direct runtime INSERT/UPDATE/DELETE on agents, and an active runtime TLS 1.3
connection. API /ready returns 200 privately; anonymous and invalid-token /agents
return 401; both existing named credentials return 200. API/PostgreSQL have no
public domains or TCP proxies. Public website and /healthz return 200, /operator
returns 401, www redirects to apex with 308, and staging hostname returns 401.
Both official domain certificates are valid.

Current usage: $22.36748839410259 / $60 shared hard limit, isOverLimit=false;
headroom approximately $37.63. These are reported usage and container ceilings,
not a forecast or guaranteed monthly cost. No extra permanent service is needed.
No worker service exists; worker replicas must remain zero. Provider is offline;
inference, controlled work, scheduler, wallet and worker flags are false.
Factory/avatar flags are currently false. Preserve public landing=true,
SUPA_SITE_ORIGIN=https://supa-web-staging.up.railway.app, and authenticated private
API connectivity at http://supa-api.railway.internal:8080. Do not change DNS.

## Exact image compatibility and security

API source: `3d9188345b290a4b1a28e48208b29307802b2bd9`.
Approved already-published image:

`ghcr.io/pyco404/supa-agency-api-infra-3b5d53616ab8@sha256:7624248d776414548d80fe0834c9ee608b6167765dcf25463f6e4197716f4cf9`

Fresh owner-authenticated package inspection confirms Private visibility,
association with private pyco404/supa-agency-infra and the exact manifest in the
package. Anonymous access is denied (401). Retained integrity evidence verifies
configuration `sha256:03697ef0de72c776e56baf72ae589ddc26d73af7eafd7164e3940fd7a6a28d18`
and all 24 layers. No API republication or rebuild is needed.
Validation 42395421998 retained 0 CRITICAL / 0 HIGH / 34 MEDIUM / 8 LOW / 0 UNKNOWN
and 0 secrets. CVE-2026-76844 and CVE-2026-85091 are absent from that approved scan.
MEDIUM/LOW findings remain unsuppressed; this review is not a new vulnerability
scan.

Frontend source: `4dcab96e491a2ef686531dcd7efdecfcc451d397`.
Successful validation: 42367980220. Artifact:
`validated-frontend-image-42367980220`, available until November 14, 2026,
11:27:14 UTC. Expected archive SHA-256:
`23137dd454b7a7715019d518060c279ff23326977ed28915dc4052c26485e6e2`.
Configuration:
`sha256:e1756c25875112fdebf1d6500bc5d82ef11c4d9ffd143c56e65cd383783582e3`.

The retained security-report ZIP checksum and identity were freshly verified;
the full image archive must be rehashed by the publisher before any registry
write. Retained gates passed: 131 HTTP boundaries, desktop/mobile browser
acceptance, runtime/Sharp/gzip/TLS, non-root execution, packaging (59 expected
files) and network isolation. Its approved scan contains 0 CRITICAL / 0 HIGH /
1 MEDIUM (CVE-2026-4738 in zlib) / 0 LOW / 0 UNKNOWN / 0 secrets. Preserve that
finding and the existing HIGH/CRITICAL and secrets gates.

Git comparison confirms web, reference-web, Rust crates and all three SQL
extensions are identical between the frontend source and approved API source.
Retained integration covers creation, ownership, consent/revocation, concurrency,
idempotency, rollback and real database restart on disposable PostgreSQL 18.3.
Exact API image binaries/libraries passed native startup/authentication/persistence
checks; that evidence explicitly does not claim a local Docker container started.
The exact frontend/API pair has not yet passed live staging acceptance.

The matching frontend is not yet privately published. The current publisher
still pins run 42359612597 / the older About release. It must be narrowly updated
and reviewed to pin the artifact above, all configuration/layer identities and
complete security evidence. Proposed fresh package:
`ghcr.io/pyco404/supa-agency-frontend-infra-23137dd454b7`.
No immutable frontend registry digest may be guessed before publication. No
rebuild or substitution with a newer local application HEAD is intended; recent
local GitHub-label / text-wrap changes are outside this retained image.

## SQL ordering, ownership and minimum grants

Use the dedicated staging migration identity, not the API runtime, and verify
schema absence and exact hashes before each install. Each file contains its own
BEGIN/COMMIT and must run with ON_ERROR_STOP; these are non-idempotent extensions,
not automatic migrations. Do not edit the 13 migrations or _sqlx_migrations.

| Order | Exact file | SHA-256 |
| --- | --- | --- |
| 1 | deployment/ecosystem-social.sql | `e50180e1cdb0d103d2d8a85d84d16b8debfe8328456f4b6a8ebb3222068e2b3c` |
| 2 | deployment/agent-factory.sql | `902ed919f74b9b56fb0a7cf3dc62b063cc3db404efbeb4a656da40cfdae5bd56` |
| 3 | deployment/agent-avatars.sql | `94610db26d3b592f16638a0c0d8c312737f13d0a88d179acb7e5611b0496b3cd` |

Hashes match both approved source and existing review evidence. Dependencies:
ecosystem private purpose/consent and immutable evidence first; Factory atomic
submissions and owner ACL projections second; avatar storage/revisions third.
Fourteen new tables bring the expected total from 50 to 64; no new sequence is
introduced. Fixed pg_catalog search paths protect SECURITY DEFINER functions;
PUBLIC feature-schema/table/function privileges are revoked. Function ownership
must remain with the reviewed migration role. No qualification reviewer policy,
token-registry extension or broad ecosystem-social-grants.sql installation.

After all three installs pass, apply only this additional runtime grant set in
one separately checked transaction:

```sql
BEGIN;
GRANT USAGE ON SCHEMA agent_factory, agent_avatars TO supa_staging_runtime;
GRANT EXECUTE ON FUNCTION
 agent_factory.create_private(uuid,uuid,jsonb,jsonb,jsonb,jsonb),
 agent_factory.owned(uuid), agent_factory.managed(uuid),
 agent_avatars.command(uuid,text,jsonb),
 agent_avatars.create_agent(uuid,uuid,jsonb,jsonb,jsonb,jsonb,uuid)
TO supa_staging_runtime;
COMMIT;
```

Recheck required existing SELECT on application_epoch, agent_access and
_sqlx_migrations. Add no direct table writes, role memberships, schema CREATE,
migration authority or financial permissions. Compare all original grants and
migration checksums again. Run effective-runtime positive reads/function calls
and negative DML/DDL/financial/migration/privilege-escalation checks.

## Synthetic identities and activation configuration

Existing synthetic operator/viewer credentials remain valid until November 8,
2026, 09:29:05 UTC / 10:29:05 WAT. They are not Factory creators. Preserve their
roles, tokens and access. If expired before required testing, stop; bounded
renewal requires separate approval.

Prepare two new synthetic application principals with separate UUIDs/tokens and
explicit backend Role::Owner only for their own newly created pilot agents.
These are not PostgreSQL roles, platform-owner grants or public registrations.
Grant no access to existing agents, records or reviewer policies. Explicitly map
both principal IDs to deadlines no more than 24 hours after provisioning; absent
deadlines preserve legacy behavior in code and therefore are unacceptable here.
Append named credentials in SUPA_OPERATOR_CREDENTIALS and deadlines in
SUPA_OPERATOR_SESSION_DEADLINES without replacing existing entries. Add matching
frontend-only account bindings in SUPA_WEB_OPERATORS. Generate/store secrets only
through private stdin/secret-management paths; no secrets in reports or Git.

Set SUPA_SYNTHETIC_FACTORY_ENABLED=true only on API and web for the approved
pilot. Enabling SUPA_AGENT_AVATARS_ENABLED=true on both also requires explicit
rollout scope; it uses the reviewed image normalization and private bytea storage,
not paid generation. Default avatars remain optional/non-AI. Keep the private
service token and all existing unrelated credentials unchanged. API private
staging verifies schema instead of running automatic migrations. Its dedicated
synthetic branch starts no scheduler, wallet service or worker monitor.

Browser sessions are HttpOnly, Secure, SameSite=Strict and at most 15 minutes;
backend deadlines are independently enforced. Same-origin writes and server-bound
principal identity remain required. My Agents lists creator-owned pilot records
only, with revocation rechecked. Agents are private, paused, pending qualification,
zero balance/spending authority and have empty enabled capabilities. Requested
capabilities are review intent. Execution/lifecycle and token-launch controls
remain disabled, regardless of a synthetic account's role.

## Exact execution sequence — requires separate approval

Before any staging mutation, finish and independently verify the retained
frontend's private publication. This preparatory gate prevents leaving staging
partially upgraded because a publication fails; it does not deploy the image.
Use the existing publisher, a fresh private destination, checksum verification,
configuration/all-layer integrity, owner-verified repository association and
anonymous denial. Native Railway pulls use existing encrypted credentials;
never request a manual GHCR token. Failed pull access stops the rollout.

Then, during the approved staging window:

1. Recheck target, healthy services, remaining spend, session validity and absence
   of feature schemas. Reverify the retained recovery bundle checksum and its
   source fingerprint, excluding documented normal heartbeat timestamps. If
   source data/schema drift invalidates that baseline, stop for backup review;
   do not silently export or restore again. Keep old deployment digests recorded.
2. Apply ecosystem-social, agent-factory, then agent-avatars, checking each
   transactional result and owner definitions. Stop on any mismatch; do not
   automatically retry non-idempotent installers or drop schemas.
3. Apply the exact five EXECUTE / two-schema USAGE grants above. Verify 64 tables,
   six sequences, original checksums, PUBLIC revocations and restricted privileges.
   Keep Factory disabled if any check fails.
4. Privately provision the two bounded synthetic account bindings/deadlines.
   Stage API configuration and its approved immutable image together for one
   controlled API replacement, avoiding a credential-only automatic deployment.
   Enable only the agreed synthetic Factory/avatar flags. Verify private /ready,
   TLS runtime access, identity binding and owner/operator/viewer denials.
5. On the existing supa-web only, stage the matching verified immutable frontend
   and approved pilot account/flag configuration together for one replacement.
   Preserve public browsing, staging origin/authentication, API URL, registry
   credentials, domains and resource ceilings. Require the existing controller's
   bounded read-only observations, identity checks and complete browser acceptance.
6. Desktop/mobile: Sign In → Factory → review → Create Agent → My Agents →
   owner detail. Exercise default avatar and an optional normalized upload, refresh,
   duplicate-key replay/conflict, separate-owner/anonymous/viewer denials and
   consent revocation. Keep public listings empty unless explicitly approved;
   public browsing must not request private backend data or perform writes.
7. Verify persisted IDs, private configuration, paused status and zero authority.
   One separately scoped API-only restart after creation may prove process-restart
   persistence and renewed owner isolation; do not restart live PostgreSQL. Its
   storage restart test already passed in recovery; extension-level restart and
   concurrency/transaction-failure evidence comes from disposable tests. If a new
   live-database restart is requested, stop for separate authorization.
8. Recheck protected operator/staging routes, cross-origin denial, disabled
   execution, quotas/storage, CPU/memory and spend. Preserve synthetic evidence
   and immutable receipts. Stop on confirmed critical failure; no automatic retry.

Rollback first closes synthetic Factory/avatar access and restores the prior
frontend image/configuration. Leave additive schemas and paused synthetic records,
ACL/audit, submissions and image history intact; do not delete evidence or restore
an older database automatically. Prior API recovery image is
`ghcr.io/pyco404/supa-agent-api-infra-a64d5174d695@sha256:cdd82965e82eca8e283c560b2f8caacdc764a7d2b8e6d59a334c31b088901736`;
prior frontend is
`ghcr.io/pyco404/supa-agency-frontend-infra-bdd136be4baf@sha256:d4aa60ec40f36f62da160369cc0183bb757aea7b9d5aec074d3a322969924419`.
Do not treat prior-API rollback against new pilot records as validated until its
compatibility is tested. Otherwise keep the new API private with the pilot off
and stop for review. Recovery configuration deployments require the rollout's
explicit safety scope. Database restore is a separate, potentially data-losing
operation and is not part of automatic application rollback.

## Remaining approval boundaries

1. Frontend publisher pin changes must be prepared/tested under one-file commits;
   separately approve their exact infrastructure push and one private publication.
   No build is needed while the approved artifact remains available.
2. After an immutable frontend reference is verified, approve the three exact SQL
   files and restricted grants, two expiring synthetic application identities,
   agreed Factory/avatar flags, one API replacement and one frontend replacement,
   plus the stated safe frontend restoration scope and API-only persistence restart
   if required. Existing credential renewal is excluded unless separately approved.
3. No real registration, real data, worker, paid inference, trading, wallet signing,
   token launch, autonomous execution or X publishing approval is implied.

The recovery environment `ff1981d9-31e5-4128-8861-88a80fd611bd`, service and volume
were verified removed; current project inventory contains only the original
staging environment and three services. The original validated backup remains
private and checksum-valid. See railway-isolated-logical-recovery-result.md.

Fresh sanitized evidence is in ignored
artifacts/functional-factory/post-recovery-rollout/. This runbook is a prepared
execution plan, not a claim that functional Factory is live or a substitute for
its mandatory staging acceptance.
