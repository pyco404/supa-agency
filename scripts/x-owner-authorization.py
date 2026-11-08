#!/usr/bin/python3 -I
"""Prepared privileged X-only gate. No credentials, OAuth, networking or enrollment.

Installation is a separate administrator-approved action. Production paths/UIDs
cannot be overridden by arguments or environment. Tests inject disposable stores.
"""
import base64
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import stat
import sys
import time
import uuid

OWNER = '6fd863e1-745d-4a06-b68e-eea69e854a69'
CALLBACK = 'http://127.0.0.1:37641/oauth/x/callback'
CUSTODY = 'supa-agency/x/supa_agency'
SCOPE = 'one-local-x-oauth-connection-and-one-budgeted-identity-read'
BASE = Path('/var/lib/supa-agent/x-owner-authorization')
TRUST = Path('/etc/supa-agent/onboarding-authority.json')
VERIFIER = Path('/usr/local/lib/supa-agent/verify-owner-evidence.py')


def require(ok):
    if not ok:
        raise ValueError('x_owner_authorization_denied')


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()


def identifier(value):
    require(isinstance(value, str) and str(uuid.UUID(value)) == value and uuid.UUID(value).int != 0)
    return value


def trusted(path, uid=0, ancestors=True):
    path = Path(path).absolute()
    for p in [path, *path.parents] if ancestors else [path]:
        info = p.lstat()
        require(not stat.S_ISLNK(info.st_mode) and info.st_uid == uid and not info.st_mode & 0o022)
    return path


def read_json(path, uid=0):
    trusted(path, uid, ancestors=(uid == 0))
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as file:
        info = os.fstat(file.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == uid and not info.st_mode & 0o022
                and info.st_nlink == 1 and info.st_size <= 32768)
        raw = file.read(32769)
        require(len(raw) <= 32768)
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result)
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=unique)


class Authority:
    def __init__(self, base, trust, verifier, uid=0):
        self.base, self.trust, self.verifier, self.uid = Path(base), Path(trust), verifier, uid

    def validate(self, receipt_id, owner, client, caller, now):
        identifier(receipt_id)
        require(owner == OWNER and isinstance(client, str) and re.fullmatch(r'[A-Za-z0-9_-]{1,200}', client))
        require(type(caller) is int and caller > 0)
        for directory in [self.base, self.base / 'receipts', self.base / 'used', self.base / 'revoked']:
            trusted(directory, self.uid, ancestors=(self.uid == 0))
            require(directory.is_dir())
        require(not os.path.lexists(self.base / 'revoked' / receipt_id))
        receipt = read_json(self.base / 'receipts' / (receipt_id + '.json'), self.uid)
        require(set(receipt) == {'version', 'receipt_id', 'owner_id', 'client_id', 'username', 'callback',
                                'custody', 'scope', 'local_uid', 'issued_at', 'expires_at', 'approved_by_reference',
                                'proof_reference', 'proof', 'authorization_approved'})
        require(type(receipt['version']) is int and receipt['version'] == 1
                and receipt['receipt_id'] == receipt_id and receipt['owner_id'] == owner
                and receipt['client_id'] == client and receipt['username'] == 'supa_agency'
                and receipt['callback'] == CALLBACK and receipt['custody'] == CUSTODY and receipt['scope'] == SCOPE
                and type(receipt['local_uid']) is int and receipt['local_uid'] == caller
                and receipt['authorization_approved'] is True
                and isinstance(receipt['approved_by_reference'], str)
                and re.fullmatch(r'[A-Za-z0-9._:/-]{1,200}', receipt['approved_by_reference']))
        issued, expires = receipt['issued_at'], receipt['expires_at']
        require(type(issued) is int and type(expires) is int and issued <= now < expires
                and 0 < expires - issued <= 600)
        proof = receipt['proof']
        require(isinstance(proof, dict) and set(proof) == {'challenge', 'owner_public_pem',
                    'custodian_public_pem', 'owner_signature_base64', 'custodian_signature_base64'})
        challenge = proof['challenge']
        # Reuse review-only proof semantics; a separate trusted receipt grants X scope.
        keys = {role: proof[role + '_public_pem'].encode('ascii') for role in ['owner', 'custodian']}
        signatures = {role: base64.b64decode(proof[role + '_signature_base64'], validate=True)
                      for role in ['owner', 'custodian']}
        evidence = self.verifier(read_json(self.trust, self.uid), challenge, keys, signatures, now)
        require(evidence['evidence_valid'] is True and evidence['grants_issued'] == 0
                and evidence['credentials_created'] is False and evidence['execution_authorized'] is False
                and receipt['proof_reference'] == evidence['challenge_sha256']
                and expires <= evidence['expires_at'])
        nonce = identifier(evidence['nonce'])
        return receipt, nonce

    def execute(self, operation, receipt_id, owner, client, caller, now=None):
        require(operation in ['consume', 'check'])
        now = int(time.time()) if now is None else now
        # Lock is installed by the administrator; never create missing authority directories.
        lock = trusted(self.base / 'authority.lock', self.uid, ancestors=(self.uid == 0))
        fd = os.open(lock, os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, 'r+b') as guard:
            info = os.fstat(guard.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == self.uid
                    and not info.st_mode & 0o022)
            fcntl.flock(guard, fcntl.LOCK_EX | fcntl.LOCK_NB)
            receipt, nonce = self.validate(receipt_id, owner, client, caller, now)
            # The proof nonce, not just the receipt ID, is single-use across receipts.
            used = self.base / 'used' / (nonce + '.json')
            marker = {'receipt_id': receipt_id, 'owner_id': owner, 'client_id': client,
                      'local_uid': caller, 'receipt_sha256': hashlib.sha256(canonical(receipt)).hexdigest()}
            if operation == 'consume':
                out = os.open(used, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
                with os.fdopen(out, 'wb') as file:
                    file.write(canonical(marker))
                    file.flush()
                    os.fsync(file.fileno())
                directory = os.open(used.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
                try: os.fsync(directory)
                finally: os.close(directory)
            else:
                require(read_json(used, self.uid) == marker)
            return {'receipt_id': receipt_id, 'owner_id': owner, 'client_id': client,
                    'expires_at': receipt['expires_at'], 'proof_nonce': nonce,
                    'consumed_nonce': True, 'publishing_enabled': False}


def main():
    signal.alarm(4)
    os.umask(0o077)
    require(os.geteuid() == 0 and len(sys.argv) == 5)
    caller_text = os.environ.get('SUDO_UID', '')
    require(caller_text.isascii() and caller_text.isdigit())
    # No workspace imports in the privileged installation.
    trusted(VERIFIER)
    require(VERIFIER.is_file())
    spec = importlib.util.spec_from_file_location('trusted_owner_evidence', VERIFIER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    result = Authority(BASE, TRUST, module.verify_document).execute(*sys.argv[1:], int(caller_text))
    print(json.dumps(result, separators=(',', ':')))


if __name__ == '__main__':
    try:
        main()
    except Exception:
        print('{"denied":"x_owner_authorization_unavailable"}', file=sys.stderr)
        sys.exit(1)
