# Routine frontend deployments

Proposed, local configuration only. Nothing here changes the running services.

**Edit frontend → push reviewed GitHub code → Railway builds/checks once → healthcheck → verify website.**

## What changes

Only existing `supa-web` in project `99648d2a-69a1-4d8c-98d0-7f2a26543c9c`,
staging `ff0b2092-e42b-4a89-ae25-441ab95d92d8`, service
`955bb925-6343-433e-b9dd-f7cbdd89114a` changes from a GHCR image source to GitHub.
The API and database retain their existing immutable image sources.

The frontend builds through `deployment/frontend-railway.Dockerfile`, derived
from the reviewed Alpine recipe. Base digest, zlib version, nonroot identity,
runtime command and management-page packaging are retained. Its separate
Dockerignore excludes private configuration, Git history, database/Rust source,
local dependencies and build output. There are no secret build arguments.

Each Railway build runs these checks against its own code and output:

1. `npm ci`, then the single Next.js build.
2. TypeScript and lint.
3. Existing secret-pattern rules against copied source, assets and built public
   chunks. Private files, symlinks, missing inputs and oversized scans fail closed.
4. Existing public-landing authentication rehearsal, extended with My Agents and
   Factory API denials. It starts temporary loopback Next.js processes with
   random synthetic credentials, checks public/staging/operator boundaries, and
   closes them. It reads no Railway credentials and connects to no database.
5. Production dependency audit, failing on HIGH/CRITICAL or unavailable results.
6. Prune development dependencies and assemble the existing nonroot runtime.

Failure prevents a deployable image from being produced. This path uses no
GitHub Actions jobs, uploaded archives, GHCR publications, OCI evidence round
trips, release dispatches or expiring live test sessions. No browser installation
is required. The existing full browser/role/ownership suites remain available for
authentication, feature, dependency and runtime changes; visual screenshot
assertions are no longer repeated as automatic rollback triggers for routine copy
or styling changes. Existing security history remains preserved.

The checked runtime previously had one unsuppressed MEDIUM zlib finding,
CVE-2026-4738. This change does not fix or suppress it. `npm audit` covers npm
packages, not OS packages. Runtime/base/lockfile changes still require a reviewed
full image vulnerability scan. Before adopting this NEW recipe, complete its
first Docker build and image scan; local smoke tests on the existing extracted
image do not substitute for that check.

## One-time adoption, requiring separate authorization

1. Review these seven configuration/tooling files and synchronize **only these
   changes** onto the approved website source. GitHub application main currently
   points at `a6c759b84c890f0f0249533f235a9f614b0adba5`. Local application main has
   187 earlier unpublished commits covering unrelated features. Do not push that
   history wholesale to enable this deployment path. Transfer the reviewed
   pipeline files onto an isolated checkout of the approved main under the
   project's one-file commit policy; do not squash/rebase existing history.
2. Complete one native-recipe build/scan and the boundary rehearsal against it.
   Confirm the first release source contains only intended frontend changes.
3. In ONLY staging `supa-web`, connect `pyco404/supa-agency`, branch `main`, root
   directory `/`. Grant the Railway GitHub App access if needed. Select config
   file `/deployment/frontend.railway.toml`. Source connection can initiate a
   build/deploy: authorization must include this first rollout.
4. Enable GitHub autodeploy for this service. Checks run INSIDE the Docker build,
   so this path needs no Actions workflow. Leave Wait for CI disabled for this
   native-build path; the required checks cannot be skipped inside the build. Do not connect
   the API, worker or PostgreSQL to this branch. Do not run the whole-project IaC.
5. Retain all current service variables, domains, private backend URL, encrypted
   credentials, one replica and 0.5 CPU/512 MiB limits. Keep the existing start
   command (Docker CMD), no pre-deploy command, and `/healthz` timeout 120 seconds.
   Do not add migrations or pass private credentials into build arguments.
6. Preserve current capability flags. This is not authorization for supervised
   execution, paid inference, real registration, trading, wallets, tokens or X.
7. Record the first native build's source SHA, deployment ID and running digest.
   Verify public routes, authenticated My Agents, videos and desktop/mobile once.

After that one-time adoption, routine reviewed pushes trigger the frontend
watch paths automatically. Backend-only commits do not trigger frontend builds.
Pushing these local files and changing the live source are NOT authorized yet.

## Verification and recovery

After a routine deploy, check `/healthz`, the nine public sections and anonymous
401 responses for `/my-agents`, `/operator` and private APIs. Browse desktop and
mobile. A visual timing mismatch is diagnostic evidence, not an automatic reason
to switch images. A confirmed authentication violation requires immediate
frontend recovery and investigation. Run full role/ownership/browser suites when
those behaviors change; never interpret incomplete checks as success.

Railway admits the new deployment after its healthcheck passes. A build or
healthcheck failure must leave the previous healthy deployment serving traffic.
This is deployment readiness, not continuous uptime monitoring.

Keep the last healthy frontend deployment/image for recovery:
`aef4c641-0dbd-4aff-bce3-b0aa492879d7`,
`ghcr.io/pyco404/supa-agency-frontend-infra-23137dd454b7@sha256:dafa15f49d6e4eb88e8c6e69f8d1ed3870ce8b39fcc85136f641fd6757aa9bcb`.
Verify rollback availability before the first rollout. Railway native rollback
retention is plan-limited, so retain the existing private GHCR recovery package.
Use the frontend-only recovery procedure if native rollback is unavailable.
Native rollback restores that deployment's variables as well as its image:
review the variable comparison before confirming it. Do not roll back API/DB.

Official references:
- https://docs.railway.com/deployments/github-autodeploys
- https://docs.railway.com/config-as-code/reference
- https://docs.railway.com/deployments/healthchecks
- https://railway.com/guides/roll-back-bad-deploy

## Local checks

`python3 scripts/test-frontend-native-release.py`

`python3 scripts/scan-secrets.py --self-test`

The context-only scanner needs the built layout (`web/src`, `web/public`,
`reference-web`, `web/.next/static`). Its normal Git/history mode is unchanged.
The ordinary pre-commit/pre-push scanner and one-file commit policy still apply.
