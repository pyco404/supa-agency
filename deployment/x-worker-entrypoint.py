#!/usr/bin/env python3
"""Non-root X-only container supervisor. Never loads dotenv or a platform database."""
import datetime
import fcntl
import hashlib
import hmac
import http.server
import json
import os
from pathlib import Path
import resource
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
import uuid

ROOT = Path('/etc/supa-agent/x-worker')
PG = Path('/usr/lib/postgresql/18/bin')
APP = Path('/opt/supa-x')
CREDENTIAL = Path('/run/credentials/supa-x-autonomy.service/oauth1.json')
SCHEMA = ('x-posting-foundation.sql', 'ecosystem-social.sql', 'social-review.sql',
          'social-autonomy.sql', 'social-first-post.sql')
KEYS = ('X_API_KEY', 'X_API_SECRET', 'X_ACCESS_TOKEN', 'X_ACCESS_SECRET')


class Denied(Exception):
    def __init__(self, category, operation=None, exit_code=None):
        super().__init__(category)
        self.diagnostic = {'category': category}
        if operation in ('initdb', 'postgres', 'pg_ctl', 'pg_isready', 'createdb', 'psql',
                         'supa-x-autonomy', 'supa-x-mock-worker'):
            self.diagnostic['operation'] = operation
        if isinstance(exit_code, int):
            self.diagnostic['exit_code'] = exit_code


def private(path, directory=False):
    m = path.lstat()
    if (m.st_uid != os.getuid() or m.st_mode & 0o077
            or not (stat.S_ISDIR(m.st_mode) if directory else stat.S_ISREG(m.st_mode))
            or (not directory and (m.st_nlink != 1 or m.st_size > 16384))):
        raise Denied('private_storage_required')


def read(path):
    private(path)
    with path.open() as f:
        return json.load(f)


def write(path, value):
    private(path.parent, True)
    if path.exists() or path.is_symlink():
        private(path)
    fd, name = tempfile.mkstemp(prefix='.pending-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as f:
            json.dump(value, f, separators=(',', ':'))
            f.flush()
            os.fsync(f.fileno())
        os.replace(name, path)
        parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
    finally:
        if os.path.exists(name):
            os.unlink(name)


class Worker:
    def __init__(self, root=ROOT, pg=PG, app=APP, environ=None):
        self.root, self.pg, self.app = root, pg, app
        self.env = dict(os.environ if environ is None else environ)
        self.kind = (app / 'bundle-mode').read_text().strip()
        self.mode = self.env.get('SUPA_X_WORKER_MODE', 'paused')
        if self.kind not in ('mock', 'live') or self.mode not in ('paused', self.kind):
            raise Denied('bundle_mode_mismatch')
        if os.getuid() == 0:
            raise Denied('non_root_required')
        private(root, True)  # An owner must prepare the volume; no root override.
        self.child_env = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', 'TZ': 'UTC'}
        self.data, self.socket = root / 'pgdata', root / 'socket'
        self.control, self.manifest = root / 'control.json', root / 'ledger.json'
        self.lock, self.postgres, self.worker = None, None, None
        self.stopping = False
        self.health = {'state': 'starting', 'database_ready': False}
        self.server = None
        self.next_tick = 0
        self.pg_env = dict(self.child_env, PGHOST=str(self.socket), PGPORT='55483',
                           PGUSER='supa_x_mock_admin', PGDATABASE='postgres',
                           PGPASSFILE='/dev/null', PGSERVICEFILE='/dev/null')

    def command(self, argv, env=None, timeout=30, capture=False):
        try:
            result = subprocess.run([str(a) for a in argv], env=env or self.child_env,
                                    stdout=subprocess.PIPE if capture else subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                    timeout=timeout, pass_fds=(() if self.lock is None else (self.lock,)))
        except (OSError, subprocess.TimeoutExpired):
            raise Denied('child_operation_unconfirmed', Path(argv[0]).name) from None
        if result.returncode:
            raise Denied('child_operation_failed', Path(argv[0]).name, result.returncode)
        if capture:
            if len(result.stdout) > 16384:
                raise Denied('bounded_report_required')
            return json.loads(result.stdout)

    def acquire(self):
        path = self.root / 'publisher.lock'
        self.lock = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        private(path)
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            os.close(self.lock)
            self.lock = None
            raise Denied('publisher_already_running') from None
        lease_path = self.root / 'lease.json'
        old = read(lease_path) if lease_path.exists() else {}
        self.lease = {'holder': str(uuid.uuid4()), 'generation': int(old.get('generation', 0)) + 1,
                      'state': 'held', 'expires_unix': int(time.time()) + 30}
        write(lease_path, self.lease)

    def prepare(self):
        previous = read(self.control) if self.control.exists() else {}
        self.socket.mkdir(mode=0o700, exist_ok=True)
        private(self.socket, True)
        fresh = not self.data.exists()
        if self.kind == 'live':
            # No initialization, restore, grant or ledger migration in live mode.
            receipt = read(self.manifest)
            if receipt.get('version') != 1 or receipt.get('mode') != 'live' or receipt.get('import_verified') is not True:
                raise Denied('verified_import_required')
            if fresh or (self.data / 'PG_VERSION').read_text().strip() != '18':
                raise Denied('imported_pg18_required')
            self.child_env['DATABASE_URL'] = self.live_database_url()
            # Live mounts retain PostgreSQL SCRAM configuration; never rewrite it.
            self.configure_credentials()
        else:
            if any(k in self.env for k in KEYS) or self.env.get('DATABASE_URL'):
                raise Denied('mock_rejects_live_configuration')
            if fresh:
                if self.manifest.exists():
                    raise Denied('missing_existing_ledger')
                self.command([self.pg / 'initdb', '-D', self.data, '-U', 'supa_x_mock_admin',
                              '--auth-local=trust', '--auth-host=reject', '--encoding=UTF8',
                              '--locale=C.UTF-8', '--no-instructions'])
                # Only the mock-only restricted login can connect over loopback.
                # Insert the explicit mock rule before initdb's reject-all rules.
                hba = self.data / 'pg_hba.conf'
                hba.write_text('host supa_x_first_post supa_x_post_runtime 127.0.0.1/32 trust\n' + hba.read_text())
            else:
                if read(self.manifest).get('mode') != 'mock':
                    raise Denied('mock_volume_mismatch')
                if (self.data / 'PG_VERSION').read_text().strip() != '18':
                    raise Denied('pg18_required')
            self.child_env['DATABASE_URL'] = 'postgresql://supa_x_post_runtime@127.0.0.1:55483/supa_x_first_post'
        private(self.data, True)
        pgargs = [self.pg / 'postgres', '-D', self.data, '-p', '55483', '-k', self.socket,
                  '-c', 'listen_addresses=127.0.0.1', '-c', 'shared_buffers=64MB',
                  '-c', 'max_connections=10', '-c', 'work_mem=2MB', '-c', 'max_wal_size=64MB',
                  '-c', 'min_wal_size=32MB', '-c', 'log_statement=none',
                  '-c', 'log_min_error_statement=panic', '-c', 'log_min_messages=fatal']
        self.postgres = subprocess.Popen([str(a) for a in pgargs], env=self.child_env,
                                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                         start_new_session=True, pass_fds=(self.lock,))
        deadline = time.monotonic() + 30
        while not self.database_ready():
            if self.postgres.poll() is not None or time.monotonic() >= deadline or self.stopping:
                raise Denied('postgres_startup_failed')
            time.sleep(0.1)
        if fresh and self.kind == 'mock':
            self.command([self.pg / 'createdb', 'supa_x_first_post'], self.pg_env)
            for name in (*SCHEMA, 'x-worker-mock.sql'):
                self.command([self.pg / 'psql', '-X', '-v', 'ON_ERROR_STOP=1', '-d', 'supa_x_first_post',
                              '-f', self.app / 'sql' / name], self.pg_env)
            expires = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(hours=24)
            write(self.manifest, {'version': 1, 'mode': 'mock', 'expires_at': expires.isoformat()})
            self.child_env['SUPA_X_MOCK_EXPIRES_AT'] = expires.isoformat()
            self.dispatch('authorize')  # Mock grant only, created paused.
        if self.kind == 'mock':
            self.child_env['SUPA_X_MOCK_EXPIRES_AT'] = read(self.manifest)['expires_at']
            self.dispatch('recover')
            state = self.dispatch('read', capture=True)
            if state.get('state') in ('active', 'paused'):
                self.dispatch('pause')
        else:
            imported = read(self.manifest)
            grants = imported.get('grant_ids')
            if not isinstance(grants, list) or len(grants) > 128:
                raise Denied('imported_grant_inventory_required')
            for grant in grants:
                try:
                    if not isinstance(grant, str) or uuid.UUID(grant).int == 0:
                        raise ValueError()
                except ValueError:
                    raise Denied('invalid_imported_grant') from None
                self.command([self.app / 'bin' / 'supa-x-autonomy', 'recover-paused', grant], self.child_env)
        # Fresh/mismatched/expired authority always starts paused. A previously
        # owner-resumed exact grant may continue only after safe durable recovery;
        # quarantine/terminal state or changed authority blocks this continuation.
        write(self.control, {'active': False})
        try:
            if self.mode != 'paused' and previous.get('active') is True and previous == self.active_marker():
                self.dispatch('resume')
                write(self.control, previous)
        except (Denied, OSError, ValueError, KeyError):
            write(self.control, {'active': False, 'reason': 'unconfirmed'})
        self.health = {'state': 'paused', 'database_ready': True}

    def live_database_url(self):
        url = self.env.get('DATABASE_URL', '')
        try:
            parsed = urllib.parse.urlsplit(url)
            valid = (parsed.scheme in ('postgres', 'postgresql') and parsed.hostname == '127.0.0.1'
                     and parsed.port == 55483 and parsed.path == '/supa_x_first_post'
                     and parsed.username == 'supa_x_post_runtime' and not parsed.query and not parsed.fragment)
        except ValueError:
            valid = False
        if not valid:
            raise Denied('dedicated_loopback_ledger_required')
        return url

    def configure_credentials(self):
        present = [k in self.env for k in KEYS]
        if any(present):
            if not all(present):
                raise Denied('incomplete_or_inactive_credentials')
            private(CREDENTIAL.parent, True)
            if CREDENTIAL.exists() or CREDENTIAL.is_symlink():
                existing = read(CREDENTIAL)
                if set(existing) != set(KEYS) or not all(isinstance(existing[k], str) and
                        hmac.compare_digest(existing[k].encode(), self.env[k].encode()) for k in KEYS):
                    raise Denied('credential_mount_would_be_overwritten')
            else:
                write(CREDENTIAL, {k: self.env[k] for k in KEYS})
        # No credentials in PostgreSQL, Rust or control-command environment.
        for key in KEYS:
            self.env.pop(key, None)
            os.environ.pop(key, None)

    def binary_args(self, operation):
        if self.kind == 'mock':
            return [self.app / 'bin' / 'supa-x-mock-worker', operation]
        receipt = self.env.get('SUPA_X_ACTIVATION_RECEIPT', '')
        try:
            if uuid.UUID(receipt).int == 0:
                raise ValueError()
        except ValueError:
            raise Denied('installed_authorization_required') from None
        return [self.app / 'bin' / 'supa-x-autonomy', operation, receipt]

    def dispatch(self, operation, capture=False):
        return self.command(self.binary_args(operation), self.child_env, capture=capture)

    def active_marker(self):
        if self.kind == 'mock':
            return {'active': True, 'mode': 'mock', 'expires_at': read(self.manifest)['expires_at']}
        receipt_id = str(uuid.UUID(self.env['SUPA_X_ACTIVATION_RECEIPT']))
        raw = (Path('/etc/supa-agent/x-autonomous-publishing/live') / (receipt_id + '.json')).read_bytes()
        if len(raw) > 16384:
            raise Denied('bounded_receipt_required')
        return {'active': True, 'mode': 'live', 'receipt_id': receipt_id,
                'authorization_sha256': hashlib.sha256(raw).hexdigest()}

    def database_ready(self):
        if self.postgres is None or self.postgres.poll() is not None:
            return False
        try:
            if int((self.data / 'postmaster.pid').read_text().splitlines()[0]) != self.postgres.pid:
                return False
            return subprocess.run([str(self.pg / 'pg_isready'), '-h', '127.0.0.1', '-p', '55483', '-t', '1'],
                                  env=self.child_env, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL, timeout=2).returncode == 0
        except (OSError, ValueError, IndexError, subprocess.TimeoutExpired):
            return False

    def stop_child(self):
        if self.worker is not None:
            if self.worker.poll() is None:
                self.worker.terminate()
                try:
                    self.worker.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    self.worker.kill()
                    self.worker.wait(timeout=5)
            self.worker = None

    def observe(self):
        if not self.database_ready():
            self.health = {'state': 'database_unavailable', 'database_ready': False}
            raise Denied('database_unavailable')
        control = read(self.control)
        if control.get('active') is not True or self.mode == 'paused':
            self.stop_child()
            self.health = {'state': 'paused_unconfirmed' if control.get('reason') else 'paused', 'database_ready': True}
        elif self.kind == 'mock':
            if time.monotonic() >= self.next_tick:
                try:
                    before = self.dispatch('read', capture=True)
                    if before.get('state') != 'active' or before.get('expired') is True:
                        write(self.control, {'active': False})
                        self.health = {'state': 'paused', 'database_ready': True}
                        self.heartbeat()
                        return
                    result = self.dispatch('tick', capture=True)
                    if result.get('state') in ('uncertain', 'failed'):
                        raise Denied('mock_outcome_unconfirmed')
                except Denied:
                    write(self.control, {'active': False, 'reason': 'unconfirmed'})
                    self.dispatch('recover')
                    self.health = {'state': 'paused_unconfirmed', 'database_ready': True}
                    self.heartbeat()
                    return
                self.next_tick = time.monotonic() + 60
            self.health = {'state': 'active_mock', 'database_ready': True}
        else:
            if self.worker is None:
                self.worker = subprocess.Popen([str(a) for a in self.binary_args('run')],
                                               env=self.child_env, stdout=subprocess.DEVNULL,
                                               stderr=subprocess.DEVNULL, start_new_session=True,
                                               pass_fds=(self.lock,))
            if self.worker.poll() is not None:
                # No automatic restart/resubmit after an ambiguous child exit.
                write(self.control, {'active': False, 'reason': 'unconfirmed'})
                self.worker = None
                self.health = {'state': 'paused_unconfirmed', 'database_ready': True}
                self.heartbeat()
                return
            self.health = {'state': 'active', 'database_ready': True}
        self.heartbeat()

    def heartbeat(self):
        self.lease['expires_unix'] = int(time.time()) + 30
        write(self.root / 'lease.json', self.lease)
        self.health['lease_generation'] = self.lease['generation']
        self.health['observed_unix'] = int(time.time())

    def serve_health(self):
        owner = self
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                if self.path != '/healthz':
                    self.send_error(404)
                    return
                value = dict(owner.health)
                good = bool(value.get('database_ready')) and time.time() - value.get('observed_unix', 0) < 15
                raw = json.dumps(value).encode()
                self.send_response(200 if good else 503)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Cache-Control', 'no-store')
                self.send_header('Content-Length', str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
            def log_message(self, *_):
                pass
        port = int(self.env.get('PORT', '8080'))
        if not 1024 <= port <= 65535:
            raise Denied('invalid_health_port')
        self.server = http.server.ThreadingHTTPServer(('0.0.0.0', port), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def shutdown(self):
        try:
            self.stop_child()
            if self.postgres is not None and self.postgres.poll() is None:
                try:
                    self.command([self.pg / 'pg_ctl', '-D', self.data, '-m', 'fast', '-w', '-t', '15', 'stop'], timeout=20)
                finally:
                    if self.postgres.poll() is None:
                        self.postgres.terminate()
                    self.postgres.wait(timeout=5)
        finally:
            if self.server:
                self.server.shutdown()
                self.server.server_close()
            if self.lock is not None:
                self.lease.update(state='released', expires_unix=int(time.time()))
                write(self.root / 'lease.json', self.lease)
                os.close(self.lock)
                self.lock = None

    def run(self):
        self.acquire()
        try:
            self.prepare()
            self.serve_health()
            while not self.stopping:
                self.observe()
                time.sleep(1)
        finally:
            self.shutdown()

    def control_operation(self, operation):
        if operation not in ('authorize', 'check', 'resume', 'pause', 'stop', 'revoke', 'read', 'recover', 'tick-uncertain'):
            raise Denied('unknown_control')
        if operation == 'tick-uncertain' and self.kind != 'mock':
            raise Denied('mock_operation_only')
        if operation == 'resume' and self.mode == 'paused':
            raise Denied('explicit_mode_required')
        if self.kind == 'mock':
            if read(self.manifest).get('mode') != 'mock':
                raise Denied('mock_volume_mismatch')
            if self.env.get('DATABASE_URL') or any(k in self.env for k in KEYS):
                raise Denied('mock_rejects_live_configuration')
            self.child_env['DATABASE_URL'] = 'postgresql://supa_x_post_runtime@127.0.0.1:55483/supa_x_first_post'
            self.child_env['SUPA_X_MOCK_EXPIRES_AT'] = read(self.manifest)['expires_at']
        else:
            # Control commands reuse the same target/identity checks in Rust.
            self.child_env['DATABASE_URL'] = self.live_database_url()
        if operation in ('pause', 'stop', 'revoke'):
            write(self.control, {'active': False})
        result = self.dispatch(operation, capture=operation == 'read')
        if self.kind == 'live' and operation in ('authorize', 'resume'):
            # The Rust command just verified this exact root-controlled receipt.
            # Track its grant so a later boot can safely recover it after expiry.
            receipt_id = str(uuid.UUID(self.env['SUPA_X_ACTIVATION_RECEIPT']))
            receipt = json.loads((Path('/etc/supa-agent/x-autonomous-publishing/live') / (receipt_id + '.json')).read_text())
            grant = str(uuid.UUID(receipt['grant']['grant_id']))
            imported = read(self.manifest)
            ids = imported.get('grant_ids')
            if not isinstance(ids, list) or len(ids) >= 128:
                raise Denied('bounded_grant_inventory_required')
            if grant not in ids:
                imported['grant_ids'] = ids + [grant]
                write(self.manifest, imported)
        if operation == 'resume':
            write(self.control, self.active_marker())
        print(json.dumps({'operation': operation, 'recorded': True, 'mode': self.kind,
                          'report': result if operation == 'read' else None}))


def main():
    os.umask(0o077)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    worker = Worker()
    args = sys.argv[1:]
    if args:
        if len(args) != 2 or args[0] != 'control':
            raise Denied('invalid_arguments')
        worker.control_operation(args[1])
    else:
        def stop(*_):
            worker.stopping = True
        signal.signal(signal.SIGTERM, stop)
        signal.signal(signal.SIGINT, stop)
        worker.run()


if __name__ == '__main__':
    try:
        main()
    except (Denied, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        # No raw stderr, command arguments, paths, URLs or environment values.
        diagnostic = error.diagnostic if isinstance(error, Denied) else {'category': 'configuration_or_storage_unavailable'}
        print(json.dumps(dict(diagnostic, state='stopped_unconfirmed', automatic_retry=False)), file=sys.stderr)
        sys.exit(1)
