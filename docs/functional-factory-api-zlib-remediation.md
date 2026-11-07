# Functional Factory API: maintained zlib security update

2026-11-07. Local candidate only; no push, CI retry, publication or Railway changes.

## Failure and maintained correction

[API validation 42367971072](https://github.com/pyco404/supa-agency-infra/actions/runs/42367971072) built application `4dcab96e491a2ef686531dcd7efdecfcc451d397` but correctly blocked retention: 0 CRITICAL, 2 HIGH, 35 MEDIUM, 8 LOW, 0 UNKNOWN and 0 detected secrets. Both HIGH findings, CVE-2026-76844 and CVE-2026-85091, concern `zlib1g` `1:1.3.dfsg+really1.3.1-1+b1`. The failed image is not approved or retained as a validated deployment archive.

Debian now records a supported Trixie security fix for [CVE-2026-76844](https://security-tracker.debian.org/tracker/CVE-2026-76844) and [CVE-2026-85091](https://security-tracker.debian.org/tracker/CVE-2026-85091): `1:1.3.dfsg+really1.3.1-1+deb13u1`. This supersedes the earlier observation that no stable fix was available. No private backport, unstable package or exception is proposed.

`deployment/api.Dockerfile` retains the current pinned Distroless runtime and Rust source/build. A digest-pinned official Debian Trixie stage uses normal authenticated APT to download the exact security package, checks its SHA-256, extracts its full payload and copies its actual control/md5 metadata into Distroless's existing `status.d` layout. The shared library and package inventory are both replaced. Build tools, APT and the shell remain outside the final runtime. The worker and frontend recipes are unchanged.

| Reviewed input | Identity |
| --- | --- |
| Official Debian preparation-stage index | `sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2` |
| Debian preparation-stage amd64 manifest | `sha256:7378260c0a098d27ac206225db5e7d92b8c7c84ea7afccc86eeb513756bbd392` |
| Security package | `zlib1g_1.3.dfsg+really1.3.1-1+deb13u1_amd64.deb` |
| Package SHA-256 | `8c27c342230503b73dfde629642c3b352e87b554e06d60cf58772f236a305ceb` |
| Patched `libz.so.1.3.1` SHA-256 | `6c13931760210e8b391b63a337dfe4071346c4ad6e3827694edb7b6989bb16dd` |
| Existing Distroless amd64 manifest, unchanged | `sha256:65af2e9e44e71de2df28245f6814301775d3ef04436b7d563f240d86fedf24aa` |

The preparation stage explicitly rejects architectures other than amd64, matching the existing workflow. Exact-version/checksum pins intentionally fail if upstream availability changes. No automatic alternate version or source is allowed.

## Local evidence and limits

Private evidence remains under `artifacts/security/api-zlib-debian-security/`. The official security `InRelease` signatures were verified using Debian's published archive keys; its SHA-256 authenticates the Packages index, which authenticates the downloaded package. The matching development package supplied extracted test headers only, with no workstation installation. Public signing fingerprints were checked against [Debian's key documentation](https://ftp-master.debian.org/keys.html).

Actual package tests passed:

- Package name, version, architecture and reviewed SHA-256; SONAME `libz.so.1`, unchanged dependency on `libc.so.6`.
- Original Distroless exported versioned symbols remain available in the fixed library.
- A compiled probe actually loads the fixed shared library, recovers from the synthetic `/dev/full` gzip-write failure and completes normal gzip write/read roundtrips.
- The cached API binary loads using the actual digest-verified Distroless loader/library closure and rejects deliberately invalid configuration before any database/provider connection. Its direct dependencies are libc, libm, libgcc and the loader, not libz. This limits demonstrated direct exposure; it does not waive a shipped vulnerable package.
- Nine focused recipe/integrity regression tests and 44 existing staging-security regressions pass. Changed package bytes and symlink package inputs are rejected.

These are native tests with extracted vendor files, not a built-container certification. Docker is unavailable locally. Fresh container startup, verified PostgreSQL TLS, image identity, full vulnerability and embedded-secret scanning remain required. Scanner recognition is **unverified**: Debian's fixed status is not a claim of zero findings in an assembled image. Keep all findings unsuppressed and fail on any HIGH/CRITICAL or secret finding, even if a scanner database has not yet learned this fix.

Reproduce the package tests after independent signed-index verification:

```sh
python3 scripts/test-api-zlib-security.py \
  --package artifacts/security/api-zlib-debian-security/zlib.deb \
  --original-library artifacts/security/api-zlib-debian-security/original-libz.so.1.3.1 \
  --include-dir artifacts/security/api-zlib-debian-security/headers/usr/include
python3 scripts/test-staging-validation.py
```

## Reuse and next approval

[Frontend validation 42367980220](https://github.com/pyco404/supa-agency-infra/actions/runs/42367980220) remains valid for its exact source: retain `validated-frontend-image-42367980220`, archive SHA-256 `23137dd454b7a7715019d518060c279ff23326977ed28915dc4052c26485e6e2`, configuration `sha256:e1756c25875112fdebf1d6500bc5d82ef11c4d9ffd143c56e65cd383783582e3`. Its one MEDIUM finding CVE-2026-4738 remains unsuppressed; all other severity counts and secrets were zero. There is no frontend source change in this remediation and no reason for another frontend build.

Next requires separate approval to push the exact local application/infrastructure commits and execute **one API-only build and security scan**, pinned to the new application revision. Do not publish or deploy on that approval. A failed gate stops without automatic retries.

Existing Factory schema extensions, narrow grants, independent encrypted backup/recovery, synthetic identity installation and staging rollout approvals remain separate, as described in infrastructure `docs/functional-factory-staging.md`. No database, live API/frontend, credential, DNS, financial record or disabled execution capability was changed.

## Build-helper correction after run 42390415424

[The one authorized remediation run](https://github.com/pyco404/supa-agency-infra/actions/runs/42390415424) passed source/preflight checks but stopped with `subprocess_failed_docker`. Its subprocess wrapper discarded stderr and the command identity. The retained report cannot establish the historical Docker subcommand or recover its original stderr; no scanner results or validated archive exist. Do not report unavailable findings as zero or claim a successful remediation scan.

An isolated APT reproduction using the authenticated security index establishes a deterministic defect in the reviewed Docker build stage. `apt-get download` saves the package as `zlib1g_1%3a1.3.dfsg+really1.3.1-1+deb13u1_amd64.deb` (encoded epoch), while the recipe's checksum/extraction commands expected the upstream pool filename without the epoch. The actual downloaded bytes match the approved SHA-256, but the old checksum command exits 1 because its named file is missing. This confirms a build-stage defect consistent with the early CI failure; it does not prove no earlier registry/network failure occurred in that historical run.

The narrow correction downloads into a fresh dedicated directory, requires exactly one amd64 zlib package, and normalizes its filename to `zlib1g.deb` before the unchanged checksum, extraction and metadata-copy steps. Zero or multiple matches fail. The package version, checksum, vendor provenance, API source behavior, base digests, non-root identity and security gates are unchanged.

The complete corrected Dockerfile RUN command was executed locally with only the two APT commands substituted: update is a no-op, download supplies the previously authenticated exact vendor package under APT's actual filename. Real shell, checksum, dpkg extraction, library identity and inventory checks passed. This is a package-stage reproduction, not an image build. Initial reproduction-harness issues (path remapping and a shell function name unsupported by dash) were corrected in the ignored harness; no application assertion was removed.

Infrastructure now assigns each Docker invocation a fixed operation name and retains exit code/category, elapsed time, timeout information and a bounded sanitized stderr excerpt. It never records argv, stdout, stdin or environment dumps. Credentials, known secret values, authentication lines, URLs, key material and sensitive paths are redacted before truncation. Failures produce a private `docker-failure.json` covered by the existing report upload; if retention itself fails, the original error still appears in sanitized console JSON. Unknown errors remain failures. No Docker or deployment retry was introduced.

Cleanup of a known unstarted inspection container remains mandatory. A cleanup failure fails validation and cannot mask the first copy/inspection failure. Regression tests execute the actual subprocess and main error-reporting paths with isolated synthetic Docker executables, including bounded timeout, redaction, evidence retention, missing-tool, cleanup and unchanged security-gate cases. Rust API library tests also pass. Private local evidence: `artifacts/security/api-zlib-docker-diagnosis/`.

No CI rerun, image build, publication, deployment, package installation, PostgreSQL operation, credential change or DNS change occurred in this correction. Keep the already validated frontend artifact unchanged. Next requires review and separate approval to synchronize the exact new application/infrastructure heads and execute one API-only validation. Fresh image identity, startup compatibility and full unsuppressed vulnerability/secrets results remain required before staging acceptance.
