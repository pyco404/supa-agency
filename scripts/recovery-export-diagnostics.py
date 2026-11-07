#!/usr/bin/env python3
"""Allowlisted diagnostics for private PostgreSQL export observations.

Never return raw process output, SQL, paths, URLs, credentials, rows or arbitrary
identifiers. Only fixed diagnostic templates and validated numeric statuses leave
this module. An unknown message is represented by a digest, never assumed safe.
"""
import hashlib
import re

# Fixed templates preserve useful cause information without redacting arbitrary
# user-controlled text and then accidentally treating the remainder as safe.
PATTERNS = {
    'permission_denied': r'permission denied|insufficient privilege',
    'table_owner_required': r'must be owner of (?:table|relation|sequence|schema)',
    'role_assumption_denied': r'permission denied to set role|must be member of role',
    'client_server_version_mismatch': r'version mismatch|aborting because of server version',
    'invalid_snapshot': r'invalid snapshot identifier|snapshot.*does not exist|invalid snapshot data',
    'snapshot_isolation_mismatch': r'snapshot.*isolation|snapshot-importing transaction|cannot import.*snapshot',
    'snapshot_read_only_mismatch': r'read-only.*snapshot|snapshot.*read-only',
    'read_only_transaction': r'cannot execute.*read-only transaction',
    'connection_failed': r'connection to server.*failed|could not connect|connection refused|could not translate host',
    'role_connection_limit': r'too many connections for role',
    'server_connection_limit': r'remaining connection slots|too many clients already',
    'connection_lost': r'server closed the connection|connection.*(?:lost|reset)|broken pipe|SSL SYSCALL',
    'tls_verification_failed': r'certificate verify failed|server certificate|root certificate|SSL error|TLS error',
    'authentication_failed': r'password authentication failed|no password supplied|authentication failed|no pg_hba.conf entry',
    'expired_identity': r'password has expired|role.*not permitted to log in',
    'statement_timeout': r'canceling statement due to statement timeout',
    'lock_timeout': r'lock timeout|could not obtain lock',
    'snapshot_session_timeout': r'idle-in-transaction timeout|idle_in_transaction_session_timeout',
    'output_permission_denied': r'could not open output file.*permission denied|could not open output file',
    'disk_full': r'no space left on device|disk quota exceeded',
    'file_size_limit': r'file size limit exceeded|file too large',
    'executable_unavailable': r'command not found|no such file or directory|failed to run command',
    'option_rejected': r'unrecognized option|invalid option|invalid compression|unsupported compression|not supported',
    'archive_invalid': r'not.*valid archive|could not read from input file|decompress|corrupt|invalid archive',
    'row_security': r'row-level security|row security',
    'query_failed': r'query failed|query was:',
    'postgresql_error': r'ERROR:|FATAL:',
    'client_error': r'pg_dump: error:|pg_restore: error:',
    'client_warning': r'pg_dump: warning:|pg_restore: warning:',
}


def timeout_argv():
    """Same 90-second TERM deadline on GNU coreutils and Railway BusyBox."""
    return ['timeout', '-s', 'TERM', '90']


def diagnose(text, operation, exit_status):
    if operation not in {'pg_dump', 'pg_restore_list', 'pg_restore_readback', 'ssh_transport'}:
        raise ValueError('unreviewed_operation')
    if not isinstance(exit_status, int) or not -128 <= exit_status <= 255:
        raise ValueError('invalid_exit_status')
    if not isinstance(text, str):
        raise ValueError('text_required')
    # Inspect a bounded prefix/tail, but never emit any substring of either.
    sample = text[:32768] + text[-32768:] if len(text) > 65536 else text
    categories = [name for name, pattern in PATTERNS.items() if re.search(pattern, sample, re.I | re.S)]
    if exit_status and not categories:
        categories = ['unknown_client_error']
    return {'operation': operation, 'exit_status': exit_status, 'categories': categories,
            'diagnostic_sha256': hashlib.sha256(text.encode()).hexdigest(),
            'diagnostic_bytes': len(text.encode()), 'raw_output_retained': False}


def protocol(text):
    """Read only fixed remote markers from either SSH output channel."""
    found = {}
    for name in ('DUMP_EXIT_CODE', 'DUMP_ERROR_BYTES', 'DUMP_ERROR_SHA256', 'DUMP_CATEGORY',
                 'ARCHIVE_READBACK_EXIT_CODE', 'FINAL_STAGE', 'CLEANUP_ROLES'):
        values = re.findall(r'^' + name + r'=([a-zA-Z0-9_-]{1,96})\s*$', text, re.M)
        if name in {'DUMP_EXIT_CODE', 'ARCHIVE_READBACK_EXIT_CODE'}:
            values = [v for v in values if v.isdigit() and 0 <= int(v) <= 255]
        elif name == 'DUMP_ERROR_BYTES':
            values = [v for v in values if v.isdigit()]
        elif name == 'DUMP_ERROR_SHA256':
            values = [v for v in values if re.fullmatch('[a-f0-9]{64}', v)]
        elif name == 'DUMP_CATEGORY':
            allowed = set(PATTERNS) | {'owner_required', 'certificate_verification_failed',
                                      'output_file_error', 'pg_dump_client_error',
                                      'pg_dump_client_warning', 'wrapper_error'}
            values = [v for v in values if v in allowed]
        elif name == 'CLEANUP_ROLES':
            values = [v for v in values if v in {'SUCCESS', 'FAILED'}]
        elif name == 'FINAL_STAGE':
            allowed = {'identity', 'baseline', 'create_temporary_roles',
                       'effective_permission_checks', 'snapshot', 'single_export',
                       'archive_integrity', 'role_cleanup', 'source_integrity',
                       'bundle_size_integrity', 'source_export_complete'}
            values = [v for v in values if v in allowed]
        if values:
            found[name.lower()] = values[-64:]
    return found
