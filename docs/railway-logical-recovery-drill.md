# Railway staging logical-recovery drill — stopped at preflight

Date: November 7, 2026. No export, restore, restart, resource creation, credential
change, database mutation or deployment occurred. No automatic retry was made.
This is not successful logical recovery or native snapshot recovery evidence.

## Verified read-only control-plane observations

Project `99648d2a-69a1-4d8c-98d0-7f2a26543c9c`, staging environment
`ff0b2092-e42b-4a89-ae25-441ab95d92d8`, PostgreSQL service
`77352ca9-435e-4870-b092-58365310c0f3` (`supa-postgres`). PostgreSQL deployment
`db546a74-4038-4536-b450-d93e78362808` remains SUCCESS on its approved image;
API and frontend also report SUCCESS. PostgreSQL/API have no public domains or
TCP proxies. The approved source service and volume were not changed.

Three non-expired native backups remain listed. Latest:
`b7c738c3-d3be-4613-8177-051f40ab7269`, created November 7 at 17:25 UTC,
expires November 13 at 17:25 UTC. This is availability metadata, not restore proof.
Workspace usage observed: $22.124508 against the existing $60 hard limit;
`isOverLimit=false`. Temporary resources were never provisioned, so there are no
incremental recovery-instance compute/storage charges. Control-plane/SSH traffic
was not separately metered; no exact incremental invoice amount is claimed.
Execution/inference flags remain disabled, provider offline, worker service absent;
Factory/avatar feature flags remain disabled. Existing synthetic sessions were
unexpired at inspection. No session or credential value was retained or disclosed.

## Preflight failure and local correction

One read-only SQL probe was attempted over the existing verified-TLS connection,
with default_transaction_read_only enabled and statement timeout 15 seconds.
The psql process returned exit code 3. The wrapper did not retain its stderr, so
no exact server error message or SQLSTATE is claimed.

Local inspection identified a defect in the new role-permissions SELECT: 19
opening parentheses and only 18 closing parentheses outside SQL string literals.
The query was syntactically incomplete. This is a probe defect, not evidence of
an unhealthy PostgreSQL server or failed authorization. The local ignored helper
was corrected and Python compilation passed; it has not been rerun. Diagnostic
retention now permits only bounded known error categories, not raw SQL output.

Current SQL health, synthetic-only data scope and least-privilege export access
are not verified by this attempt. Prior bootstrap reports cannot substitute for
fresh checks. No administrator export, role creation or expanded grants were used
as a fallback. No backup bytes were created, so size, SHA-256, restore, migration/
schema/permission comparison and restart persistence remain untested.

## Cleanup and next gate

Temporary environment/service/volume/credentials: none. Cleanup required: none.
No sensitive dump exists. Sanitized observations and local helper are under
ignored `artifacts/railway-logical-recovery-drill/`; existing metadata observations
are retained in their ignored evidence directories.

Next authorization: one corrected read-only preflight attempt. If it passes,
resume the already scoped single recovery drill: create only uniquely recorded
new recovery environment/service/volume IDs, cap clone resources at 0.5 vCPU /
512 MiB, target one hour, and retain the 16,777,216-byte complete-bundle limit.
Stop rather than split, increase the limit or retry a failed export/restore.
Record new resource IDs before operations; restart only the recovery instance;
remove only those recorded drill resources after verification, including temporary
credentials and dump material. Preserve sanitized results. If no minimally
privileged complete export role exists, stop and request separately scoped role/
grant approval rather than use the administrator or broaden runtime access.

Synthetic-only staging integration remains NO-GO pending successful recovery.
Independent encrypted off-platform recovery remains mandatory before real-user
onboarding or sensitive production records. Native snapshots were not restored
or certified by this attempt.
