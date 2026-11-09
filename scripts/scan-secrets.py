#!/usr/bin/env python3
"""Redacted local Git gate: staged blobs, reachable history, known local secrets and key formats.

No third-party scanner is installed. This targeted scanner is a gate, not proof that
arbitrary unknown secrets cannot exist. Findings contain paths/rules only.
"""
import argparse
import base64
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
GIT = [str(ROOT / 'scripts/git-local')]
PATTERNS = {
    'openai_key': re.compile(rb'\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{24,}'),
    'github_token': re.compile(rb'\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})'),
    'aws_access_key': re.compile(rb'\b(?:AKIA|ASIA)[A-Z0-9]{16}\b'),
    'private_key_pem': re.compile(rb'-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----'),
    'credential_url': re.compile(rb'(?:postgres(?:ql)?|https?)://[^\s:/]+:([^\s@/]+)@'),
    'secret_assignment': re.compile(rb'(?i)(?:api[_-]?key|access[_-]?token|private[_-]?key|master[_-]?key|password|secret)\s*["\x27]?\s*[:=]\s*["\x27]([A-Za-z0-9_+/=-]{24,})'),
}
PLACEHOLDERS = {b'change-me', b'your-local-password', b'password', b'local-password', b'your-password'}
SECRET_NAME = re.compile(r'(?i)(api.?key|secret|password|private.?key|api.?token|access.?token|database.?url)$')

def git(*args, input=None):
    result = subprocess.run(GIT + list(args), cwd=ROOT, input=input, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise RuntimeError('git_scan_read_failed')
    return result.stdout


def history_snapshots(commits):
    # Compare every complete snapshot to the empty tree, including merge commits.
    # One Git process replaces a process per commit; no persistent scan cache.
    empty = git('hash-object', '-t', 'tree', '--stdin', input=b'').strip()
    trees = git('log', '--no-walk=unsorted', '--format=%T', *commits).splitlines() if commits else []
    if len(trees) != len(commits) or any(not re.fullmatch(rb'[0-9a-f]{40}', tree) for tree in trees):
        raise RuntimeError('invalid_history_tree_inventory')
    requests = b''.join(empty + b' ' + tree + b'\n' for tree in trees)
    snapshots = git('diff-tree', '--stdin', '--no-commit-id', '--raw', '--no-renames', '--no-abbrev',
                    '-r', '-z', input=requests)
    if requests and not snapshots:
        raise RuntimeError('missing_history_snapshots')
    entries = snapshots.split(b'\0')
    index = 0
    while index < len(entries):
        metadata = entries[index]
        if not metadata:
            if index != len(entries) - 1:
                raise RuntimeError('invalid_history_snapshot')
            break
        # Git prefixes each snapshot with its requested tree/commit pair.
        metadata = metadata.rsplit(b'\n', 1)[-1]
        if not metadata and index == len(entries) - 1:
            break
        fields = metadata.split()
        if len(fields) != 5 or fields[0] != b':000000' or fields[4] != b'A':
            raise RuntimeError('invalid_history_snapshot')
        if index + 1 >= len(entries) or not entries[index + 1]:
            raise RuntimeError('invalid_history_path')
        mode, oid = fields[1].decode(), fields[3].decode()
        yield mode, 'commit' if mode == '160000' else 'blob', oid, entries[index + 1].decode()
        index += 2


def read_blobs(oids):
    requests = b''.join(oid.encode() + b'\n' for oid in oids)
    data = git('cat-file', '--batch', input=requests)
    offset = 0
    for oid in oids:
        end = data.index(b'\n', offset)
        fields = data[offset:end].split()
        if len(fields) != 3 or fields[:2] != [oid.encode(), b'blob']:
            raise RuntimeError('invalid_scan_blob')
        size = int(fields[2])
        start = end + 1
        offset = start + size
        if size < 0 or data[offset:offset + 1] != b'\n':
            raise RuntimeError('invalid_scan_blob_size')
        yield oid, data[start:offset]
        offset += 1
    if offset != len(data):
        raise RuntimeError('unexpected_scan_blob_data')

def entropy(value):
    return -sum((value.count(c) / len(value)) * math.log2(value.count(c) / len(value)) for c in set(value))

def rules(data, known):
    found = set()
    for name, pattern in PATTERNS.items():
        if name == 'secret_assignment':
            # Every assignment alternative contains one of these literal words.
            # Avoid the costly case-insensitive regex on unrelated binary data.
            lower = data.lower()
            if not any(word in lower for word in (b'key', b'token', b'password', b'secret')):
                continue
        for match in pattern.finditer(data):
            if name == 'credential_url' and match[1] in PLACEHOLDERS:
                continue
            if name == 'secret_assignment' and entropy(match[1]) < 4.0:
                continue
            found.add(name)
    if any(value in data for value in known):
        found.add('known_local_secret')
    # Solana CLI keypair arrays (64 u8 integers), regardless of filename.
    try:
        decoded = json.loads(data)
        if isinstance(decoded, list) and len(decoded) == 64 and all(type(x) is int and 0 <= x <= 255 for x in decoded):
            found.add('solana_private_key_array')
        if isinstance(decoded, dict) and {'ciphertext', 'nonce', 'wallet_id'} <= decoded.keys():
            found.add('encrypted_wallet_envelope')
    except (ValueError, UnicodeDecodeError):
        pass
    return sorted(found)

def protected(path):
    parts = Path(path).parts
    return any(x in {'.git', '.git-recovery', '.supa-secrets', '.supa-vault', '.aws', '.codex', '.agents', 'pgdata', 'postgres-data', 'node_modules', '.next', '.browser-profile', 'test-results', 'playwright-report'} for x in parts) or any(x.startswith('.env') and x != '.env.example' for x in parts) or Path(path).suffix in {'.pem', '.key', '.p12', '.pfx', '.db', '.sqlite', '.sqlite3', '.dump', '.backup'} or path.endswith('.sql.gz')

def local_secrets():
    known = []
    values = dict(os.environ)
    # Read only for exact-match detection; never echo values or copy them into reports.
    for path in [*ROOT.glob('.env*'), *ROOT.joinpath('web').glob('.env*')]:
        if not path.is_file() or path.name == '.env.example':
            continue
        for line in path.read_text().splitlines():
            match = re.match(r'(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)', line)
            if match:
                values[match[1]] = match[2].strip().strip('"\x27')
    for key, value in values.items():
        if SECRET_NAME.search(key) and len(value) >= 12:
            match = re.search(r'://[^:/]+:([^@/]+)@', value)
            # Passwordless local database addresses are configuration, not secret values.
            if 'DATABASE' not in key.upper() or match:
                known.append(value.encode())
            if match and len(match[1]) >= 12 and match[1].encode() not in PLACEHOLDERS:
                known.append(match[1].encode())
    custody=ROOT/'.supa-secrets'
    if custody.is_dir():
        for path in custody.iterdir():
            if path.is_file() and not path.is_symlink() and path.stat().st_size==32:
                raw=path.read_bytes()
                known.extend([raw,base64.b64encode(raw),raw.hex().encode()])
    return known

def scan():
    known = local_secrets()
    findings = []
    staged = git('ls-files', '--stage', '-z').split(b'\0')
    blob_paths = {}
    for entry in staged:
        if not entry:
            continue
        metadata, path = entry.split(b'\t', 1)
        path = path.decode()
        mode, oid, stage = metadata.decode().split()
        if protected(path) or mode not in {'100644', '100755'} or stage != '0':
            findings.append({'path': path, 'rule': 'protected_path_or_nonregular_file'})
        blob_paths.setdefault(oid, path)
    # All reachable commit trees, including historical versions of deleted/renamed files.
    commits = git('rev-list', '--all').decode().splitlines()
    history_entries = 0
    checked_history = set()
    for mode, kind, oid, path in history_snapshots(commits):
        history_entries += 1
        identity = (mode, kind, path)
        if identity not in checked_history:
            checked_history.add(identity)
            if protected(path) or kind != 'blob' or mode not in {'100644', '100755'}:
                findings.append({'path': path, 'rule': 'historical_protected_path_or_nonregular_file'})
        if kind == 'blob':
            blob_paths.setdefault(oid, path)
    for oid, data in read_blobs(blob_paths):
        for rule in rules(data, known):
            findings.append({'path': blob_paths[oid], 'rule': rule})
    # Verify local sensitive paths remain ignored even if they do not exist yet.
    ignored = ['.env', '.env.eval', '.env.devnet', '.env.wallet', '.supa-secrets/devnet-master', '.supa-vault/devnet/wallet.json', '.git-recovery/config', 'local.db', 'local.sqlite3', 'data.dump', 'pgdata/PG_VERSION', 'web/.env.local', 'web/.browser-profile/Default/Cookies', 'web/.next/server/private.json', 'web/test-results/trace.zip', 'web/node_modules/package/index.js']
    for path in ignored:
        result = subprocess.run(GIT + ['check-ignore', '-q', '--no-index', path], cwd=ROOT)
        if result.returncode:
            findings.append({'path': path, 'rule': 'sensitive_path_not_ignored'})
    return {'scanner': 'local-pattern-and-known-secret-gate', 'staged_entries': sum(bool(e) for e in staged), 'reachable_commits': len(commits), 'history_tree_entries': history_entries, 'unique_blobs_scanned': len(blob_paths), 'known_local_secret_matching': True, 'sensitive_ignore_checks': len(ignored), 'findings': findings, 'passed': not findings, 'limitations': 'Targeted local scan; no third-party scanner installed; unknown secret formats and semantic disclosure require human review.'}

def self_test():
    assert rules(b'sk-proj-' + b'A' * 40, []) == ['openai_key']
    assert 'known_local_secret' in rules(b'prefix' + b'private-synthetic-value', [b'private-synthetic-value'])
    assert 'solana_private_key_array' in rules(json.dumps([1] * 64).encode(), [])
    assert 'private_key_pem' in rules(b'-----BEGIN ' + b'PRIVATE KEY-----', [])
    assert 'credential_url' in rules(b''.join([b'postgresql://', b'user', b':', b'actual-secret', b'@localhost/db']), [])
    assert not rules(b'postgresql://supa:your-local-password@localhost/db', [])
    assert protected('.env.eval') and protected('.supa-vault/devnet/x.json') and protected('local.db') and protected('backup.sql.gz') and not protected('.env.example') and not protected('migrations/0004_wallet.sql')
    assert protected('web/.env.local') and protected('web/.browser-profile/Default/Cookies') and protected('web/.next/server/private.json') and protected('web/test-results/trace.zip') and not protected('web/tests/fixtures.ts')
    value = b'AbCdEfGhIjKlMnOpQrStUvWxYz0123456789'
    for name in (b'API_KEY', b'access-token', b'privateKey', b'master_key', b'PASSWORD', b'SeCrEt'):
        assert 'secret_assignment' in rules(name + b'="' + value + b'"', [])
    assert not rules(b'\x00\xffunrelated binary payload', [])
    print(json.dumps({'self_tests_passed': 15}))

def frontend_context(root):
    """Scan a Docker frontend workspace without Git metadata or private dotenv access."""
    root = root.resolve(strict=True)
    findings, count, total = [], 0, 0
    for name in ('.env', 'web/.env', 'web/.env.local'):
        if (root / name).exists():
            findings.append({'path': name, 'rule': 'private_configuration_in_build_context'})
    for name in ('web/src', 'web/public', 'reference-web', 'web/.next/static'):
        base = root / name
        if not base.exists() or base.is_symlink():
            raise RuntimeError('frontend_scan_input_missing')
        for path in sorted(base.rglob('*')):
            relative = path.relative_to(root).as_posix()
            if path.is_symlink():
                raise RuntimeError('frontend_scan_symlink_rejected')
            if not path.is_file():
                continue
            count += 1
            size = path.stat().st_size
            total += size
            if count > 10000 or size > 64 * 1024 * 1024 or total > 256 * 1024 * 1024:
                raise RuntimeError('frontend_scan_limit_exceeded')
            # Built public chunks are intentionally scanned here; Git policy
            # continues to forbid tracking .next output in the ordinary mode.
            checked = relative.removeprefix('web/.next/static/') if name == 'web/.next/static' else relative
            if protected(checked):
                findings.append({'path': relative, 'rule': 'private_file_in_frontend'})
                continue
            for rule in rules(path.read_bytes(), []):
                findings.append({'path': relative, 'rule': rule})
    return {'scanner': 'frontend-source-and-client-bundle-pattern-gate',
            'files_scanned': count, 'findings': findings, 'passed': not findings,
            'limitations': 'Pattern checks supplement default-deny Docker context and pre-push history scanning; no production secrets are loaded.'}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--self-test', action='store_true')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--frontend-context', type=Path)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    try:
        report = frontend_context(args.frontend_context) if args.frontend_context else scan()
    except Exception:
        print(json.dumps({'passed': False, 'error': 'scanner_failed_without_disclosing_private_values'}))
        raise SystemExit(1)
    output = json.dumps(report, indent=2) + '\n'
    if args.output:
        args.output.write_text(output)
    print(output, end='')
    if not report['passed']:
        raise SystemExit(1)

if __name__ == '__main__':
    main()
