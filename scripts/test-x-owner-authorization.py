#!/usr/bin/env python3
"""Disposable synthetic proofs; no installed trust, real keys, sudo, keyring or X."""
import base64
from concurrent.futures import ThreadPoolExecutor
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]
def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
m = load('x_owner_gate', ROOT / 'scripts/x-owner-authorization.py')
proofs = load('owner_proof_fixtures', ROOT / 'scripts/test-owner-evidence.py')


class GateTests(unittest.TestCase):
    def setUp(self):
        self.proof = proofs.ProofTests()
        self.proof.setUp()
        self.addCleanup(self.proof.doCleanups)
        self.base = self.proof.d / 'authority'
        self.base.mkdir(mode=0o700)
        for name in ['receipts', 'revoked', 'used']:
            (self.base / name).mkdir(mode=0o700)
        (self.base / 'authority.lock').touch(mode=0o600)
        self.trust = self.proof.d / 'trust.json'
        self.trust.write_text(json.dumps(self.proof.trust))
        self.trust.chmod(0o600)
        self.id = str(uuid.uuid4())
        self.receipt = {'version': 1, 'receipt_id': self.id, 'owner_id': m.OWNER, 'client_id': 'fixture_client',
            'username': 'supa_agency', 'callback': m.CALLBACK, 'custody': m.CUSTODY, 'scope': m.SCOPE,
            'local_uid': os.getuid(), 'issued_at': 1000, 'expires_at': 1090,
            'approved_by_reference': 'synthetic-admin-check', 'authorization_approved': True,
            'proof_reference': __import__('hashlib').sha256(proofs.m.canonical(self.proof.challenge)).hexdigest(),
            'proof': {'challenge': self.proof.challenge,
                      **{role + '_public_pem': value.decode('ascii') for role, value in self.proof.keys.items()},
                      **{role + '_signature_base64': base64.b64encode(value).decode('ascii')
                         for role, value in self.proof.signatures.items()}}}
        self.gate = m.Authority(self.base, self.trust, proofs.m.verify_document, os.getuid())
        self.save()

    def save(self):
        p = self.base / 'receipts' / (self.id + '.json')
        p.write_text(json.dumps(self.receipt))
        p.chmod(0o600)

    def execute(self, operation='consume', **changes):
        args = dict(operation=operation, receipt_id=self.id, owner=m.OWNER,
                    client='fixture_client', caller=os.getuid(), now=1050)
        args.update(changes)
        return self.gate.execute(**args)

    def test_valid_consumption_check_and_restart(self):
        result = self.execute()
        self.assertEqual(result['proof_nonce'], self.proof.challenge['nonce'])
        self.assertTrue(result['consumed_nonce'])
        self.assertFalse(result['publishing_enabled'])
        self.assertEqual(self.execute('check'), result)
        restarted = m.Authority(self.base, self.trust, proofs.m.verify_document, os.getuid())
        with self.assertRaises(FileExistsError):
            restarted.execute('consume', self.id, m.OWNER, 'fixture_client', os.getuid(), 1050)
        self.assertEqual(len(list((self.base / 'used').iterdir())), 1)

    def test_missing_unconsumed_and_duplicate_receipts(self):
        with self.assertRaises(OSError): self.execute('check')
        with self.assertRaises(OSError): self.execute(receipt_id=str(uuid.uuid4()))
        self.execute()
        self.id = str(uuid.uuid4())
        self.receipt['receipt_id'] = self.id
        self.save()
        with self.assertRaises(FileExistsError): self.execute()

    def test_exact_bindings_types_and_no_extra_privileges(self):
        original = copy.deepcopy(self.receipt)
        for field, value in [('owner_id', str(uuid.uuid4())), ('client_id', 'wrong'), ('username', 'wrong'),
            ('callback', m.CALLBACK.replace('127.0.0.1', 'localhost')), ('custody', 'other'), ('scope', 'publish'),
            ('local_uid', os.getuid() + 1), ('local_uid', True), ('authorization_approved', False),
            ('version', True), ('approved_by_reference', ''), ('proof_reference', '0' * 64), ('extra', True)]:
            with self.subTest(field=field, value=value):
                self.receipt = {**copy.deepcopy(original), field: value}
                self.save()
                with self.assertRaises((ValueError, KeyError)): self.execute()
                self.assertEqual(list((self.base / 'used').iterdir()), [])
        self.receipt = original
        self.save()
        for change in [{'owner': str(uuid.uuid4())}, {'client': 'wrong'}, {'caller': 0}]:
            with self.assertRaises(ValueError): self.execute(**change)

    def test_expiry_and_future_and_lifetime(self):
        for issued, expires, now in [(1000, 1050, 1050), (1060, 1090, 1050), (400, 1090, 1050),
                                     (1000, 1101, 1050), (1000, True, 1050)]:
            self.receipt.update(issued_at=issued, expires_at=expires)
            self.save()
            with self.assertRaises(ValueError): self.execute(now=now)

    def test_revocation_before_and_after_consumption(self):
        revoked = self.base / 'revoked' / self.id
        revoked.touch(mode=0o600)
        with self.assertRaises(ValueError): self.execute()
        revoked.unlink()
        self.execute()
        revoked.touch(mode=0o600)
        with self.assertRaises(ValueError): self.execute('check')

    def test_proof_forgery_review_purpose_and_trust_changes(self):
        original = copy.deepcopy(self.receipt)
        for change in ['purpose', 'signature', 'trust']:
            self.receipt = copy.deepcopy(original)
            if change == 'purpose': self.receipt['proof']['challenge']['purpose'] = 'oauth-consent'
            if change == 'signature': self.receipt['proof']['owner_signature_base64'] = base64.b64encode(b'0' * 64).decode()
            if change == 'trust':
                trust = copy.deepcopy(self.proof.trust)
                trust['owner']['independent_control_verified'] = False
                self.trust.write_text(json.dumps(trust))
            self.save()
            with self.assertRaises((ValueError, subprocess.SubprocessError)): self.execute()

    def test_consumed_record_is_bound_to_exact_approved_bytes(self):
        self.execute()
        self.receipt['approved_by_reference'] = 'changed-admin-reference'
        self.save()
        with self.assertRaises(ValueError): self.execute('check')
        (self.base / 'used' / (self.proof.challenge['nonce'] + '.json')).write_text('{}')
        with self.assertRaises(ValueError): self.execute('check')

    def test_symlink_unsafe_permissions_and_unknown_json_fields(self):
        p = self.base / 'receipts' / (self.id + '.json')
        p.chmod(0o666)
        with self.assertRaises(ValueError): self.execute()
        p.unlink()
        p.symlink_to(self.trust)
        with self.assertRaises(ValueError): self.execute()
        p.unlink()
        self.save()
        with p.open('w') as file: file.write('{"version":1,"version":1}')
        with self.assertRaises(ValueError): self.execute()
        self.save()
        with self.assertRaises(ValueError): self.execute(receipt_id='../escape')

    def test_concurrent_consume_has_exactly_one_winner(self):
        def attempt(_):
            try: self.execute(); return True
            except (ValueError, OSError): return False
        with ThreadPoolExecutor(max_workers=4) as workers:
            outcomes = list(workers.map(attempt, range(4)))
        self.assertEqual(outcomes.count(True), 1)

    def test_production_helper_has_no_unprivileged_or_environment_override(self):
        result = subprocess.run(['/usr/bin/python3', '-I', str(ROOT / 'scripts/x-owner-authorization.py'),
            'consume', self.id, m.OWNER, 'fixture_client'], capture_output=True, timeout=5,
            env={'PATH': '/usr/bin', 'SUDO_UID': str(os.getuid()), 'SUPA_X_AUTHORITY_DIR': str(self.base)})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b'')
        self.assertNotIn(str(self.base).encode(), result.stderr)
        self.assertEqual(list((self.base / 'used').iterdir()), [])


if __name__ == '__main__': unittest.main()
