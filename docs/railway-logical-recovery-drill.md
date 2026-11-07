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

## Authorized corrected preflight — November 7, 2026, 21:01–21:02 UTC

The single authorized corrected SQL attempt succeeded. Confirmed exact database
`supa_staging`, cluster `7704432314739101731`, PostgreSQL 18.6, TLS 1.3 with
verify-full CA/hostname verification, recovery=false, read-only transaction and
13 successful migration records. This count is not a fresh migration checksum
comparison or a synthetic-data inventory; those remain pending.

The mandatory minimum-export-permissions check failed:

| Existing login | Tables readable | Tables with write privileges | Other authority |
| --- | ---: | ---: | --- |
| supa_staging_admin | 50/50 | 50/50 | Superuser, role/database creation, replication, BYPASSRLS |
| supa_staging_migration | 50/50 | 50/50 | Owns 139 user relations; can modify migrated objects |
| supa_staging_runtime | 39/50 | 2/50 | No elevated flags, memberships or owned user relations |

No existing least-privilege complete export role was found. The runtime cannot
produce the complete required backup; filtering the other 11 tables is forbidden.
Admin or migration authority must not substitute silently. Read-only session
settings do not remove a role's underlying capabilities. Stopped immediately on
this failed mandatory check: no new synthetic-data queries, export, restore,
provisioning or automatic retry. No partial backup or size/hash claim exists.

Read-only control-plane checks again confirmed expected project/environment/
service identities and unchanged deployment IDs for PostgreSQL, API and frontend,
all SUCCESS. Three non-expired native backup entries remain available; source
backup contents were not restored or certified. Current usage remains reported
as $22.124508/$60, isOverLimit=false. Existing execution restrictions, private API/
PostgreSQL networking and frontend origin/configuration were observed unchanged.
No SQL mutations, source restart or role/credential changes were performed.

Recovery environment, temporary credentials, backup and restore: not created.
No temporary compute/storage was allocated, so no recovery-resource charges or
cleanup apply. Read-only control-plane/SSH traffic has no separate measured
invoice; do not claim an exact total incremental charge. No restore, migration/
schema/ACL comparison, restart persistence or full source-content fingerprint was
performed. The source remains unchanged by drill actions; physical/statistical
activity caused by normal connections is not claimed byte-identical.

### Minimum next authorization

Approve creation and later removal of one uniquely named, temporary, **NOLOGIN**
export role only inside `supa_staging`, with USAGE on the reviewed user schemas
and SELECT on the complete reviewed table/sequence inventory. No writes, object
ownership, CREATE, elevated attributes, default privileges or broad cluster-wide
`pg_read_all_data` membership. Do not widen the existing runtime or migration
roles or rotate credentials. An existing privately authenticated admin transport
can select that effective role using pg_dump's role option; verify effective
current_user, read-only access and write/DDL denials before the single export.
The administrative login remains privileged; this approach constrains the export
operation's effective role and does not describe the login as least-privileged.
No role is created or granted by this document. Review exact identifiers, ACLs,
RLS policies and inventory first, including any table-owner-only export issue;
stop rather than bypass RLS or omit tables.

Then authorize resuming the remaining preflight and previously scoped single
logical recovery drill, preserving the 16 MiB complete-bundle bound and cleanup
limited to newly recorded IDs and this temporary role. Synthetic-only integration
remains NO-GO pending successful recovery. Independent encrypted off-platform
recovery remains required before real users or sensitive production records.
