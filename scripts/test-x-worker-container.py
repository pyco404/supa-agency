#!/usr/bin/env python3
"""Real subprocess/PG18 tests on new synthetic data. Never loads dotenv/live URLs."""
import contextlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.error
import urllib.request
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('worker', ROOT / 'deployment/x-worker-entrypoint.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
PG = ROOT / 'artifacts/ecosystem-pg18-binaries/root/usr/lib/postgresql/18/bin'
BINARY = ROOT / 'target/work-review-validation/debug/supa-x-mock-worker'


class Contracts(unittest.TestCase):
    def test_private_storage_rejects_links_and_shared_permissions(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'receipt'
            module.write(path, {'fixture': True})
            self.assertEqual(module.read(path), {'fixture': True})
            path.chmod(0o644)
            with self.assertRaises(module.Denied):
                module.read(path)
            path.chmod(0o600)
            other = Path(folder) / 'link'
            other.symlink_to(path)
            with self.assertRaises(module.Denied):
                module.write(other, {})

    def worker(self, folder, mode='paused'):
        app = Path(folder) / 'app'
        app.mkdir(exist_ok=True)
        (app / 'bundle-mode').write_text('mock')
        data = Path(folder) / 'volume'
        data.mkdir(mode=0o700, exist_ok=True)
        return module.Worker(data, PG, app, {'SUPA_X_WORKER_MODE': mode})

    def test_volume_lease_excludes_duplicates_even_after_deadline_or_parent_death(self):
        with tempfile.TemporaryDirectory() as folder:
            first = self.worker(folder)
            first.acquire()
            stale = dict(first.lease, expires_unix=1)
            module.write(first.root / 'lease.json', stale)
            second = self.worker(folder)
            with self.assertRaises(module.Denied):
                second.acquire()
            # PostgreSQL/dispatch descendants inherit the descriptor: killing the
            # supervisor must not release fencing while a child can still publish.
            child = subprocess.Popen(['/bin/sleep', '10'], pass_fds=(first.lock,))
            os.close(first.lock)
            first.lock = None
            try:
                with self.assertRaises(module.Denied):
                    second.acquire()
            finally:
                child.terminate()
                child.wait(timeout=5)
            second.acquire()
            self.assertEqual(second.lease['generation'], 2)
            second.shutdown()
            self.assertEqual(module.read(second.root / 'lease.json')['state'], 'released')

    def test_mock_rejects_live_environment_before_initializing_pg(self):
        with tempfile.TemporaryDirectory() as folder:
            worker = self.worker(folder, 'mock')
            worker.env['X_API_KEY'] = 'synthetic-only'
            with self.assertRaises(module.Denied):
                worker.prepare()
            self.assertFalse(worker.data.exists())
            with self.assertRaises(module.Denied):
                module.Worker(worker.root, PG, worker.app, {'SUPA_X_WORKER_MODE': 'live'})

    def test_health_rejects_stale_or_unready_state_and_has_no_control_routes(self):
        with tempfile.TemporaryDirectory() as folder:
            worker = self.worker(folder)
            with socket.socket() as bound:
                bound.bind(('127.0.0.1', 0))
                port = bound.getsockname()[1]
            worker.env['PORT'] = str(port)
            worker.serve_health()
            url = 'http://127.0.0.1:' + str(port)
            try:
                for state in ({'database_ready': False},
                              {'database_ready': True, 'observed_unix': time.time() - 30}):
                    worker.health = state
                    with self.assertRaises(urllib.error.HTTPError) as error:
                        urllib.request.urlopen(url + '/healthz', timeout=2)
                    self.assertEqual(error.exception.code, 503)
                    error.exception.close()
                worker.health = {'database_ready': True, 'state': 'paused', 'observed_unix': int(time.time())}
                with urllib.request.urlopen(url + '/healthz', timeout=2) as response:
                    self.assertEqual(json.load(response), worker.health)
                    self.assertEqual(response.headers['Cache-Control'], 'no-store')
                with self.assertRaises(urllib.error.HTTPError) as error:
                    urllib.request.urlopen(url + '/control/resume', timeout=2)
                self.assertEqual(error.exception.code, 404)
                error.exception.close()
            finally:
                worker.shutdown()

    def test_missing_live_import_fails_closed(self):
        with tempfile.TemporaryDirectory() as folder:
            worker = self.worker(folder)
            (worker.app / 'bundle-mode').write_text('live')
            worker = module.Worker(worker.root, PG, worker.app, {})
            with self.assertRaises(FileNotFoundError):
                worker.prepare()
            self.assertFalse(worker.data.exists())

    def test_live_controls_reject_database_target_overrides_before_dispatch(self):
        with tempfile.TemporaryDirectory() as folder:
            worker = self.worker(folder)
            (worker.app / 'bundle-mode').write_text('live')
            valid = 'postgresql://supa_x_post_runtime@127.0.0.1:55483/supa_x_first_post'
            for url in (None, valid.replace('55483', '5432'), valid + '?host=other',
                        valid.replace('127.0.0.1', 'example.invalid'),
                        valid.replace('supa_x_post_runtime', 'administrator')):
                env = {} if url is None else {'DATABASE_URL': url}
                instance = module.Worker(worker.root, PG, worker.app, env)
                with self.assertRaises(module.Denied):
                    instance.control_operation('read')
            instance = module.Worker(worker.root, PG, worker.app, {'DATABASE_URL': valid})
            self.assertEqual(instance.live_database_url(), valid)

    def test_private_credentials_are_idempotent_and_never_replaced(self):
        with tempfile.TemporaryDirectory() as folder:
            worker = self.worker(folder)
            (worker.app / 'bundle-mode').write_text('live')
            values = {k: 'synthetic-value-' + str(n) for n, k in enumerate(module.KEYS)}
            credential = Path(folder) / 'credential.json'
            with patch.object(module, 'CREDENTIAL', credential):
                first = module.Worker(worker.root, PG, worker.app, values)
                first.configure_credentials()
                before = credential.read_bytes()
                stamp = credential.stat().st_mtime_ns
                second = module.Worker(worker.root, PG, worker.app, values)
                second.configure_credentials()
                self.assertEqual(credential.stat().st_mtime_ns, stamp)
                changed = dict(values, X_ACCESS_SECRET='different-synthetic-only')
                with self.assertRaises(module.Denied):
                    module.Worker(worker.root, PG, worker.app, changed).configure_credentials()
                self.assertEqual(credential.read_bytes(), before)
                self.assertFalse(any(k in second.env for k in module.KEYS))

    def test_child_diagnostics_retain_exit_without_arguments_or_stderr(self):
        with tempfile.TemporaryDirectory() as folder:
            worker = self.worker(folder)
            with self.assertRaises(module.Denied) as caught:
                worker.command(['/bin/sh', '-c', 'echo synthetic-private-fixture >&2; exit 7'])
            self.assertEqual(caught.exception.diagnostic, {'category': 'child_operation_failed', 'exit_code': 7})
            self.assertNotIn('synthetic-private-fixture', str(caught.exception))


class PostgresJourney(unittest.TestCase):
    def setup_worker(self, folder):
        app = Path(folder) / 'app'
        (app / 'bin').mkdir(parents=True)
        (app / 'sql').mkdir()
        (app / 'bundle-mode').write_text('mock')
        (app / 'bin/supa-x-mock-worker').symlink_to(BINARY)
        for name in (*module.SCHEMA, 'x-worker-mock.sql'):
            shutil.copyfile(ROOT / 'deployment' / name, app / 'sql' / name)
        data = Path(folder) / 'volume'
        data.mkdir(mode=0o700)
        return self.instance(data, app)

    def instance(self, data, app):
        worker = module.Worker(data, PG, app, {'SUPA_X_WORKER_MODE': 'mock'})
        # Extracted local PG18 clients need their matching libpq. Production image
        # uses one coherent PGDG installation and has no LD_LIBRARY_PATH override.
        lib = str(PG.parents[2] / 'x86_64-linux-gnu')
        worker.child_env['LD_LIBRARY_PATH'] = lib
        worker.pg_env['LD_LIBRARY_PATH'] = lib
        return worker

    def sql(self, worker, query):
        return subprocess.check_output([str(PG / 'psql'), '-XAt', '-v', 'ON_ERROR_STOP=1',
                                        '-d', 'supa_x_first_post', '-c', query],
                                       env=worker.pg_env, stderr=subprocess.PIPE, text=True, timeout=10).strip()

    def test_mock_start_pause_caps_expiry_uncertainty_and_actual_restart(self):
        with tempfile.TemporaryDirectory(prefix='supa-x-container-', dir='/dev/shm') as folder:
            worker = self.setup_worker(folder)
            try:
                worker.acquire()
                worker.prepare()
                worker.observe()
                self.assertEqual(self.sql(worker, 'SHOW wal_segment_size'), '16MB')
                self.assertEqual(worker.health['state'], 'paused')
                self.assertEqual(self.sql(worker, 'SELECT count(*) FROM social_autonomy.attempts'), '0')
                worker.control_operation('resume')
                worker.observe()
                self.assertEqual(self.sql(worker, "SELECT count(*) FROM social_autonomy.attempts WHERE state='posted'"), '1')
                worker.next_tick = 0
                worker.observe()
                self.assertEqual(self.sql(worker, 'SELECT count(*) FROM social_autonomy.attempts'), '1')
                worker.control_operation('pause')
                worker.observe()
                self.assertEqual(worker.health['state'], 'paused')
                # Expiry check is independent of spending/caps or uncertainty.
                self.sql(worker, "UPDATE social_autonomy.grants SET expires_at=clock_timestamp()-interval '1 second'")
                with self.assertRaises(module.Denied):
                    worker.control_operation('resume')
                self.sql(worker, "UPDATE social_autonomy.grants SET expires_at=clock_timestamp()+interval '1 hour'")
                worker.control_operation('resume')
                # Preserve synthetic history while arranging six recent attempts,
                # all beyond spacing, to isolate the rolling-cap check.
                self.sql(worker, """INSERT INTO social_autonomy.attempts(id,grant_id,account_id,text,text_sha256,decision,topics,state,created_at)
                 SELECT gen_random_uuid(),'00000000-0000-0000-0000-000000000064','1','Synthetic failed slot '||n,
                  encode(sha256(convert_to('Synthetic failed slot '||n,'UTF8')),'hex'),'{}','[]','failed',clock_timestamp()-interval '5 hours' FROM generate_series(1,5) n;
                 UPDATE social_autonomy.attempts SET created_at=clock_timestamp()-interval '5 hours';
                 UPDATE social_autonomy.grants SET next_at=clock_timestamp()-interval '1 second';""")
                worker.dispatch('tick')
                self.assertEqual(self.sql(worker, 'SELECT count(*) FROM social_autonomy.attempts'), '6')
                self.sql(worker, "UPDATE social_autonomy.attempts SET created_at=clock_timestamp()-interval '25 hours'; UPDATE social_autonomy.grants SET next_at=clock_timestamp()-interval '1 second';")
                self.sql(worker, """INSERT INTO activity(id,agent_id,kind,code,simulated) VALUES('00000000-0000-0000-0000-0000000000c9','00000000-0000-0000-0000-00000000000a','goal','added',true);
                 INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released)
                 SELECT '00000000-0000-0000-0000-00000000000a',(s->'event'->>'id')::uuid,'00000000-0000-0000-0000-000000000001',s->>'snapshot_sha256',true
                 FROM jsonb_array_elements(social_review.context('00000000-0000-0000-0000-00000000000a')->'sources') s WHERE s->'event'->>'id'='00000000-0000-0000-0000-0000000000c9';""")
                worker.dispatch('tick-uncertain')
                self.assertEqual(self.sql(worker, "SELECT count(*) FROM social_autonomy.attempts WHERE state='uncertain'"), '1')
                self.assertEqual(self.sql(worker, 'SELECT state FROM social_autonomy.grants'), 'paused')
                before = self.sql(worker, 'SELECT count(*) FROM social_autonomy.attempts')
                worker.shutdown()
                self.assertFalse((worker.data / 'postmaster.pid').exists())
                worker = self.instance(worker.root, worker.app)
                worker.acquire()
                worker.prepare()
                worker.observe()
                self.assertEqual(worker.health['state'], 'paused_unconfirmed')
                self.assertEqual(worker.lease['generation'], 2)
                self.assertEqual(self.sql(worker, 'SELECT count(*) FROM social_autonomy.attempts'), before)
                with self.assertRaises(module.Denied):
                    worker.control_operation('resume')
                self.assertEqual(self.sql(worker, "SELECT count(*) FROM social_autonomy.attempts WHERE state='uncertain'"), '1')
                self.assertEqual(self.sql(worker, "SELECT count(*) FROM social_autonomy.events WHERE sha256<>encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex')"), '0')
                # Expired live-style authority in this disposable fixture only:
                # recovery authenticates via the function-only DB role, can reduce
                # authority, and never needs a live receipt or X credential.
                owner = '6fd863e1-745d-4a06-b68e-eea69e854a69'
                self.sql(worker, "UPDATE agent_access SET principal_id='"+owner+"',granted_by='"+owner+"'; UPDATE social_review.policies SET owner_id='"+owner+"',account=jsonb_set(account,'{mode}','\"live\"'); UPDATE social_autonomy.grants SET owner_id='"+owner+"',account=jsonb_set(account,'{mode}','\"live\"'),state='active',expires_at=clock_timestamp()-interval '1 second'; UPDATE social_autonomy.attempts SET state='in_flight' WHERE state='uncertain';")
                recovery = ROOT / 'target/work-review-validation/debug/supa-x-autonomy'
                worker.command([recovery, 'recover-paused', '00000000-0000-0000-0000-000000000064'], worker.child_env)
                self.assertEqual(self.sql(worker, 'SELECT state FROM social_autonomy.grants'), 'paused')
                self.assertEqual(self.sql(worker, "SELECT count(*) FROM social_autonomy.attempts WHERE state='uncertain'"), '1')
                self.sql(worker, "UPDATE social_autonomy.grants SET state='revoked'")
                worker.command([recovery, 'recover-paused', '00000000-0000-0000-0000-000000000064'], worker.child_env)
                self.assertEqual(self.sql(worker, 'SELECT state FROM social_autonomy.grants'), 'revoked')
            finally:
                worker.shutdown()


if __name__ == '__main__':
    unittest.main()
