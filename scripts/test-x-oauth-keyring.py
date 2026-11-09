#!/usr/bin/python3
"""Synthetic-only keyring tests. Actual libsecret runs on a new private DBus bus."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('x_custody', ROOT/'scripts/x-oauth-keyring.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
OWNER = '00000000-0000-0000-0000-000000000001'
CLIENT = 'synthetic_confidential_client'
SECRET = 'synthetic-client-secret-fixture-only'


class Fake:
    def __init__(self):
        self.rows = {}

    def put(self, client, owner, kind, body):
        m.require((client, owner, kind) not in self.rows)
        self.rows[client, owner, kind] = dict(body)

    def get(self, client, owner, kind):
        m.require((client, owner, kind) in self.rows)
        return self.rows[client, owner, kind]

    def items(self, client, owner, kind):
        return [self.rows[client, owner, kind]] if (client, owner, kind) in self.rows else []


class Contracts(unittest.TestCase):
    def setup_store(self):
        store = Fake()
        values = iter([OWNER, CLIENT, SECRET, SECRET])
        result = m.enrollment(store, lambda _: next(values))
        self.assertNotIn(SECRET, json.dumps(result))
        return store

    def test_enrollment_and_scoped_backend_read(self):
        store = self.setup_store()
        self.assertTrue(m.ipc('backend-read', store, dict(owner=OWNER, client_id=CLIENT))['client_secret'] == SECRET)
        for client, owner in [('other', OWNER), (CLIENT, '00000000-0000-0000-0000-000000000002')]:
            with self.assertRaises(ValueError):
                m.ipc('backend-read', store, dict(owner=owner, client_id=client))
        duplicate = iter([OWNER, CLIENT, SECRET, SECRET])
        with self.assertRaises(ValueError):
            m.enrollment(store, lambda _: next(duplicate))

    def test_invalid_identity_secret_confirmation_and_schema(self):
        for values in [[OWNER, CLIENT, SECRET, 'different'], [OWNER, CLIENT, ' secret', ' secret'],
                       ['bad', CLIENT, SECRET, SECRET], [OWNER, '', SECRET, SECRET]]:
            iterator = iter(values)
            with self.assertRaises(ValueError):
                m.enrollment(Fake(), lambda _: next(iterator))
        store = self.setup_store()
        with self.assertRaises(ValueError):
            m.ipc('backend-read', store, dict(owner=OWNER, client_id=CLIENT, credential='not-accepted'))
        with self.assertRaises(ValueError):
            m.ipc('unapproved-operation', store, dict(owner=OWNER, client_id=CLIENT))

    def test_noninteractive_enrollment_never_reads_input_or_keyring(self):
        result = subprocess.run(['/usr/bin/python3', '-I', str(ROOT/'scripts/x-oauth-keyring.py'), 'setup'], input=SECRET.encode(), capture_output=True, timeout=3)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(SECRET.encode(), result.stdout + result.stderr)
        self.assertEqual(result.stdout, b'')

    def test_invalid_arguments_are_never_echoed(self):
        result = subprocess.run(['/usr/bin/python3', '-I', str(ROOT/'scripts/x-oauth-keyring.py'), 'setup', SECRET], capture_output=True, timeout=3)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(SECRET.encode(), result.stdout + result.stderr)

    def test_connection_requires_exact_account_scopes_expiry_and_no_overwrite(self):
        store = self.setup_store()
        body = dict(owner=OWNER, client_id=CLIENT, account_id='123', username='supa_agency',
                    scope=m.SCOPES, expires_unix=int(time.time())+600, access_token='fixture-access-token-only')
        for key, bad in [('username', 'other'), ('account_id', '0123'), ('scope', m.SCOPES+['offline.access']),
                         ('expires_unix', 1), ('access_token', 'synthetic-not-a-live-token')]:
            with self.assertRaises(ValueError):
                m.ipc('backend-store-connection', store, {**body, key: bad})
        self.assertEqual(m.ipc('backend-store-connection', store, body), {'stored': True})
        with self.assertRaises(ValueError):
            m.ipc('backend-store-connection', store, body)
        self.assertEqual(len(store.rows), 2)

    def test_private_connection_read_is_exact_scoped_expiring_and_nonmutating(self):
        store = self.setup_store()
        body = dict(owner=OWNER, client_id=CLIENT, account_id='123', username='supa_agency',
                    scope=m.SCOPES, expires_unix=int(time.time())+600, access_token='fixture-access-token-only')
        m.ipc('backend-store-connection', store, body)
        query = dict(owner=OWNER, client_id=CLIENT, account_id='123')
        self.assertEqual(m.ipc('backend-read-connection', store, query), body)
        for change in [dict(account_id='124'), dict(owner='00000000-0000-0000-0000-000000000002'),
                       dict(client_id='other'), dict(extra=True)]:
            with self.assertRaises(ValueError):
                m.ipc('backend-read-connection', store, {**query, **change})
        for key, bad in [('expires_unix', 1), ('username', 'other'), ('scope', ['tweet.write']),
                         ('owner', '00000000-0000-0000-0000-000000000002')]:
            stored = store.rows[CLIENT, OWNER, 'connection']
            original = stored[key]
            stored[key] = bad
            with self.assertRaises(ValueError):
                m.ipc('backend-read-connection', store, query)
            stored[key] = original
        self.assertEqual(len(store.rows), 2)


def isolated_service():
    assert os.environ.get('SUPA_X_KEYRING_ISOLATED_TEST') == '1'
    assert Path(os.environ['HOME']).parent.name.startswith('supa-x-keyring-')
    daemon = None

    def start(fresh=False):
        child = subprocess.Popen(['/usr/bin/gnome-keyring-daemon', '--foreground', '--components=secrets', '--unlock'],
                                 stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        child.stdin.write(b'synthetic-keyring-passphrase-only')
        child.stdin.close()
        deadline = time.monotonic()+5
        while time.monotonic() < deadline:
            try:
                if fresh:
                    check = subprocess.run(['/usr/bin/python3', '-I', str(ROOT/'scripts/x-oauth-keyring.py'), 'status'], capture_output=True, timeout=2)
                    if check.returncode != 0:
                        raise ValueError('not_ready')
                    return child, None
                store = m.Keyring()
                return child, store
            except Exception:
                time.sleep(.1)
        child.terminate()
        child.wait(timeout=3)
        raise RuntimeError('isolated_keyring_unavailable')

    try:
        daemon, store = start()
        values = iter([OWNER, CLIENT, SECRET, SECRET])
        m.enrollment(store, lambda _: next(values))
        public = json.dumps(dict(owner=OWNER, client_id=CLIENT)).encode()
        result = subprocess.run(['/usr/bin/python3', '-I', str(ROOT/'scripts/x-oauth-keyring.py'), 'backend-read'],
                                input=public, capture_output=True, timeout=7)
        assert result.returncode == 0 and json.loads(result.stdout)['client_secret'] == SECRET
        assert result.stderr == b''
        binary = os.environ.get('SUPA_X_KEYRING_RUST_TEST')
        if binary:
            binary = Path(binary).resolve()
            assert binary.parent == ROOT/'target/work-review-validation/debug/deps'
            result = subprocess.run([str(binary), '--exact', 'x_oauth::local::tests::private_keyring_bridge_reads_and_retains_only_in_disposable_session', '--ignored'], capture_output=True, timeout=20)
            assert result.returncode == 0 and b'1 passed' in result.stdout, result.stdout.decode(errors='replace')[:3000]
        duplicate = iter([OWNER, CLIENT, SECRET, SECRET])
        try:
            m.enrollment(store, lambda _: next(duplicate))
            raise AssertionError('duplicate_allowed')
        except ValueError:
            pass
        # Only this disposable keyring's encrypted files are inspected.
        assert all(all(secret not in p.read_bytes() for secret in [SECRET.encode(), b'fixture-access-token-only']) for p in Path(os.environ['HOME']).rglob('*') if p.is_file())
        daemon.terminate()
        daemon.wait(timeout=3)
        daemon, _ = start(fresh=True)
        subprocess.run(['/usr/bin/python3', '-I', __file__, '--verify-restored'], check=True, timeout=7)
        print(json.dumps(dict(isolated_keyring=True, encrypted_at_rest=True, restart_persistence=True,
                              locked_denied=True, scoped_read=True, duplicates_denied=True, real_custody_operations=0, rust_bridge_verified=bool(os.environ.get('SUPA_X_KEYRING_RUST_TEST')))))
    finally:
        if daemon is not None and daemon.poll() is None:
            daemon.terminate()
            daemon.wait(timeout=3)


if __name__ == '__main__':
    if sys.argv[1:] == ['--verify-restored']:
        assert os.environ.get('SUPA_X_KEYRING_ISOLATED_TEST') == '1'
        assert Path(os.environ['HOME']).parent.name.startswith('supa-x-keyring-')
        store = m.Keyring()
        assert store.get(CLIENT, OWNER, 'configuration')['client_secret'] == SECRET
        if os.environ.get('SUPA_X_KEYRING_RUST_TEST'):
            assert store.get(CLIENT, OWNER, 'connection')['account_id'] == '123'
        assert not store.items(CLIENT, '00000000-0000-0000-0000-000000000002', 'configuration')
        store.service.lock_sync([store.collection], None)
        check = subprocess.run(['/usr/bin/python3', '-I', str(ROOT/'scripts/x-oauth-keyring.py'), 'status'], capture_output=True, timeout=3)
        assert check.returncode != 0 and SECRET.encode() not in check.stdout + check.stderr
    elif sys.argv[1:] == ['--isolated-service']:
        isolated_service()
    else:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(Contracts)
        if not unittest.TextTestRunner().run(suite).wasSuccessful():
            sys.exit(1)
        with tempfile.TemporaryDirectory(prefix='supa-x-keyring-') as temporary:
            root = Path(temporary)
            for folder in ['home', 'runtime']:
                (root/folder).mkdir(mode=0o700)
            env = dict(PATH='/usr/bin:/bin', LANG='C.UTF-8', HOME=str(root/'home'), XDG_RUNTIME_DIR=str(root/'runtime'),
                       SUPA_X_KEYRING_ISOLATED_TEST='1')
            if os.environ.get('SUPA_X_KEYRING_RUST_TEST'):
                env['SUPA_X_KEYRING_RUST_TEST'] = os.environ['SUPA_X_KEYRING_RUST_TEST']
            subprocess.run(['/usr/bin/dbus-run-session', '--', '/usr/bin/python3', '-I', __file__, '--isolated-service'],
                           env=env, check=True, timeout=25)
