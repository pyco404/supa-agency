#!/usr/bin/env python3
"""Same one-shot process in a non-deployable executable; private PG and network namespace.

Local: python3 scripts/test-x-pilot-packaged.py --binary PATH --production-binary PATH
CI: run the same command as root inside the network-none validation image with --inside.
No real credentials, external routes, inherited database URLs or real ledger mounts.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess
import sys
import sysconfig
import tempfile
import time
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]
OWNER = '6fd863e1-745d-4a06-b68e-eea69e854a69'
ACCOUNT = {'id': '1', 'username': 'supa_agency', 'mode': 'live'}
HISTORICAL = 'Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip.'
BASE = Path('/etc/supa-agent')
AUTH = BASE / 'x-pilot'
VOLUME = BASE / 'x-worker'
APP = Path('/opt/one-shot-validation')
PG_ROOT = APP / 'pg'
PG = PG_ROOT / 'usr/lib/postgresql/18/bin'
PORT = '55483'


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def write(path, value, uid=0, mode=0o640):
    raw = value if isinstance(value, bytes) else json.dumps(value).encode()
    path.write_bytes(raw)
    path.chmod(mode)
    os.chown(path, uid, 10001 if uid == 0 else uid)
    return raw


def private_report(path, value):
    """Restrictive creation scope only; restore caller umask even on failure."""
    previous = os.umask(0o077)
    try:
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'w') as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise ValueError('private report must be a single regular file')
            os.fchmod(stream.fileno(), 0o600)
            stream.truncate(0)
            stream.write(json.dumps(value, indent=2)+'\n')
    finally:
        os.umask(previous)


def fixture_directory(path, mode=0o755, uid=0):
    """Only called inside the disposable namespace, never on host directories."""
    path.mkdir(exist_ok=True, mode=mode)
    assert not path.is_symlink()
    os.chown(path, uid, uid)
    path.chmod(mode)
    return path


def drop_privileges():
    os.setgroups([])
    os.setgid(10001)
    os.setuid(10001)
    assert ctypes.CDLL(None).prctl(38, 1, 0, 0, 0) == 0  # no_new_privs


def env(pg=False):
    value = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', 'HOME': '/tmp',
             'PGPASSFILE': '/dev/null', 'PGSERVICEFILE': '/dev/null'}
    if pg:
        value['LD_LIBRARY_PATH'] = str(PG_ROOT / 'usr/lib/x86_64-linux-gnu')
    return value


def command(args, *, pg=False, allowed=(0,), data=None, timeout=20):
    p = subprocess.run([str(x) for x in args], input=data, text=True, capture_output=True,
                       env=env(pg), cwd='/tmp', preexec_fn=drop_privileges, timeout=timeout)
    if p.returncode not in allowed:
        raise AssertionError('fixture command failed: '+Path(args[0]).name+' '+str(p.returncode)+' '+p.stderr[-1200:])
    return p


def sql(query, database='supa_x_first_post', deny=False):
    return command([PG/'psql', '-XAtq', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1',
                    '-p', PORT, '-U', 'supa_pilot_admin', '-d', database],
                   pg=True, allowed=(1, 3) if deny else (0,), data=query).stdout.strip()


class Packaged(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        assert os.getuid() == 0 and set(os.listdir('/sys/class/net')) == {'lo'}
        assert not Path('/run/credentials/supa-x-autonomy.service/oauth1.json').exists()
        cls.data = Path('/tmp/supa-pilot-packaged-data')
        cls.socket = Path('/tmp/supa-pilot-packaged-socket')
        cls.socket.mkdir(mode=0o700); os.chown(cls.socket, 10001, 10001)
        command([PG/'initdb', '-D', cls.data, '-U', 'supa_pilot_admin', '--auth-local=trust', '--auth-host=trust', '--no-instructions'], pg=True)
        command([PG/'pg_ctl', '-D', cls.data, '-l', '/tmp/packaged-postgres.log', '-o', '-k '+str(cls.socket)+' -p '+PORT+' -c listen_addresses=127.0.0.1', '-w', 'start'], pg=True)
        sql('CREATE ROLE supa_x_post_runtime LOGIN;', 'postgres')
        cls.cluster = sql('SELECT system_identifier::text FROM pg_control_system()', 'postgres')

    @classmethod
    def tearDownClass(cls):
        command([PG/'pg_ctl', '-D', cls.data, '-m', 'fast', '-w', 'stop'], pg=True)

    def setUp(self):
        command([PG/'createdb', '-h', '127.0.0.1', '-p', PORT, '-U', 'supa_pilot_admin', 'supa_x_first_post'], pg=True)
        self.addCleanup(lambda: command([PG/'dropdb', '-h', '127.0.0.1', '-p', PORT, '-U', 'supa_pilot_admin', 'supa_x_first_post'], pg=True))
        for name in ('x-posting-foundation.sql', 'ecosystem-social.sql', 'social-review.sql', 'social-autonomy.sql', 'social-first-post.sql', 'x-worker-mock.sql', 'social-pilot-post.sql'):
            text = (APP/'sql'/name).read_text()
            if name == 'x-worker-mock.sql':
                text = text.replace('CREATE ROLE supa_x_post_runtime LOGIN;', '')
                text = text.replace('00000000-0000-0000-0000-000000000001', OWNER)
            sql(text)
        sql("GRANT EXECUTE ON FUNCTION social_review.pilot_post(uuid,text,jsonb),social_review.pilot_cluster() TO supa_x_post_runtime; UPDATE social_review.policies SET account=jsonb_set(account,'{mode}','\"live\"');")
        for path in (VOLUME/'offline-wire.jsonl', AUTH/'stop'):
            if path.exists(): path.unlink()
        for path in (AUTH/'approved').glob('*.json'): path.unlink()
        self.ident = str(uuid.uuid4())
        self.text = 'Synthetic packaged one-shot validation only.'
        self.scenario = 'success'
        self.receipt_mutation = {}
        self.draft_mutation = {}

    def provision(self):
        context = {'version': 1, 'image_digest': 'sha256:'+digest((APP/'offline').read_bytes()),
                   'binary_sha256': digest((APP/'offline').read_bytes()), 'ledger_cluster_id': self.cluster,
                   'ledger_import_sha256': digest(b'disposable synthetic fixture'), 'runtime_uid': 10001}
        raw = write(AUTH/'context.json', context)
        now = int(time.time())
        receipt = {'version':1, 'authorization_id':self.ident, 'task_id':str(uuid.uuid4()), 'owner_id':OWNER,
                   'account':ACCOUNT, 'text_sha256':digest(self.text.encode()), 'identity_receipt_sha256':digest(b'synthetic identity'),
                   'context_sha256':digest(raw), 'issued_unix':now, 'expires_unix':now+300,
                   'max_identity_gets':1, 'max_posts':1, 'budget_microusd':25000,
                   'approved':True, 'read_write_confirmed':True, 'credits_confirmed':True}
        receipt.update(self.receipt_mutation)
        receipt_path = AUTH/'approved'/(self.ident+'.json')
        raw = write(receipt_path, receipt)
        self.assertEqual((receipt_path.stat().st_uid, receipt_path.stat().st_gid, stat.S_IMODE(receipt_path.stat().st_mode)), (0, 10001, 0o640))
        draft = {'version':1, 'text':self.text, 'text_sha256':digest(self.text.encode()), 'account':ACCOUNT}
        draft.update(self.draft_mutation)
        write(VOLUME/'pilot-draft.json', draft, 10001, 0o600)
        write(AUTH/'offline-case.json', {'scenario':self.scenario})
        q = lambda x: "'"+str(x).replace("'", "''")+"'"
        sql('INSERT INTO social_review.pilot_approvals(authorization_id,task_id,owner_id,account,text_sha256,receipt_sha256,expires_at) VALUES('+','.join([q(self.ident),q(receipt['task_id']),q(OWNER),q(json.dumps(ACCOUNT))+'::jsonb',q(receipt['text_sha256']),q(digest(raw)), 'to_timestamp('+str(receipt['expires_unix'])+')'])+');')

    def launch(self, *, production=False):
        e = env(); e['DATABASE_URL'] = 'postgresql://supa_x_post_runtime@127.0.0.1:'+PORT+'/supa_x_first_post'
        return subprocess.Popen([str(APP/('production' if production else 'offline')), 'run-once', self.ident],
                                env=e, cwd='/tmp', preexec_fn=drop_privileges, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    def run_once(self, expected=0):
        p = self.launch(); out, err = p.communicate(timeout=10)
        self.assertEqual(p.returncode, expected, 'one-shot status mismatch: '+err)
        return out

    def routes(self):
        path = VOLUME/'offline-wire.jsonl'
        return [json.loads(line)['route'] for line in path.read_text().splitlines()] if path.exists() else []

    def assert_audit(self):
        self.assertEqual(sql("SELECT count(*) FROM (SELECT *,coalesce(lag(sha256) OVER(PARTITION BY agent_id ORDER BY id),repeat('0',64)) expected FROM social_autonomy.events) e WHERE previous_sha256<>expected OR sha256<>encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex')"), '0')
        for statement in ('TRUNCATE social_autonomy.events', 'DELETE FROM social_autonomy.events', "UPDATE social_autonomy.events SET sha256=repeat('0',64)"):
            result = command([PG/'psql','-XAtq','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',PORT,'-U','supa_pilot_admin','-d','supa_x_first_post'], pg=True, allowed=(1,3), data=statement)
            self.assertIn('ecosystem_social_history_immutable', result.stderr)

    def test_success_terminates_and_restart_does_not_replay(self):
        self.provision(); self.assertIn('posted', self.run_once())
        self.assertEqual(self.routes(), ['/2/users/me','/2/tweets'])
        self.assertEqual(sql('SELECT count(*) FROM social_review.attempts'), '1')
        self.assertEqual(sql('SELECT state FROM social_review.attempts'), 'posted')
        command([PG/'pg_ctl','-D',self.data,'-m','fast','-w','restart','-l','/tmp/packaged-postgres.log'], pg=True)
        self.assertIn('duplicate_or_recovery', self.run_once())
        self.assertEqual(self.routes(), ['/2/users/me','/2/tweets']); self.assert_audit()

    def test_missing_authorization(self):
        self.run_once(1); self.assertEqual(self.routes(), [])

    def test_expired_authorization(self):
        self.receipt_mutation = {'issued_unix':int(time.time())-600, 'expires_unix':int(time.time())-1}
        self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_wrong_account_authorization(self):
        self.receipt_mutation = {'account':dict(ACCOUNT, username='other')}
        self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_wrong_owner(self):
        self.receipt_mutation = {'owner_id':str(uuid.uuid4())}
        self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_draft_hash_mismatch(self):
        self.draft_mutation = {'text_sha256':'a'*64}
        self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_historical_content(self):
        self.text = HISTORICAL; self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_spending_ceiling(self):
        self.receipt_mutation = {'budget_microusd':25001}
        self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_identity_lookup_ceiling(self):
        self.receipt_mutation = {'max_identity_gets':2}
        self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_post_ceiling(self):
        self.receipt_mutation = {'max_posts':2}
        self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_ten_minute_ceiling(self):
        now = int(time.time())
        self.receipt_mutation = {'issued_unix':now, 'expires_unix':now+601}
        self.provision(); self.run_once(1); self.assertEqual(self.routes(), [])

    def test_wrong_observed_identity_prevents_post(self):
        self.scenario = 'wrong-identity'; self.provision(); self.run_once(1)
        self.assertEqual(self.routes(), ['/2/users/me'])

    def test_uncertainty_quarantined_across_restart(self):
        self.scenario = 'uncertain'; self.provision(); self.run_once(1)
        self.assertEqual(sql('SELECT state FROM social_review.attempts'), 'uncertain')
        self.run_once(); self.assertEqual(self.routes(), ['/2/users/me','/2/tweets']); self.assert_audit()

    def test_emergency_stop(self):
        self.provision(); write(AUTH/'stop', b'synthetic stop')
        self.run_once(1); self.assertEqual(self.routes(), [])

    def test_shutdown_after_claim_preserves_uncertainty(self):
        self.scenario = 'wait-post'; self.provision(); p = self.launch()
        try:
            deadline = time.monotonic()+5
            while len(self.routes())<2 and time.monotonic()<deadline and p.poll() is None: time.sleep(0.02)
            self.assertEqual(self.routes(), ['/2/users/me','/2/tweets'])
            p.send_signal(signal.SIGTERM); p.communicate(timeout=5); self.assertEqual(p.returncode, 1)
            self.assertEqual(sql('SELECT state FROM social_review.attempts'), 'uncertain')
            self.run_once(); self.assertEqual(len(self.routes()),2)
        finally:
            if p.poll() is None: p.terminate(); p.wait(timeout=5)

    def test_production_cannot_select_offline_transport(self):
        self.provision(); p = self.launch(production=True); p.communicate(timeout=5)
        self.assertEqual(p.returncode,1); self.assertEqual(self.routes(), [])

    def test_runtime_owned_receipt_is_denied(self):
        self.provision()
        os.chown(AUTH/'approved'/(self.ident+'.json'),10001,10001)
        self.run_once(1); self.assertEqual(self.routes(), [])

    def test_runtime_owned_receipt_ancestor_is_denied(self):
        self.provision()
        parent = AUTH/'approved'
        os.chown(parent, 10001, 10001)
        try:
            self.run_once(1); self.assertEqual(self.routes(), [])
        finally:
            os.chown(parent, 0, 0)

    def test_writable_receipt_is_denied(self):
        self.provision()
        (AUTH/'approved'/(self.ident+'.json')).chmod(0o666)
        self.run_once(1); self.assertEqual(self.routes(), [])

    def test_symlink_receipt_is_denied(self):
        self.provision()
        receipt = AUTH/'approved'/(self.ident+'.json')
        receipt.unlink(); receipt.symlink_to('/no-such-fixture')
        self.run_once(1); self.assertEqual(self.routes(), [])

    def test_default_disabled(self):
        p = command([APP/'offline','check-disabled'])
        self.assertTrue(json.loads(p.stdout)['paused']); self.assertEqual(self.routes(), [])

    def test_no_scheduler_command(self):
        p = command([APP/'offline','run'], allowed=(1,))
        self.assertNotEqual(p.returncode,0); self.assertEqual(self.routes(), [])


class Permissions(unittest.TestCase):
    def test_fixture_and_report_permissions(self):
        for path in (Path('/'), Path('/etc'), BASE, AUTH, AUTH/'approved', AUTH/'revoked', APP):
            info = path.stat()
            self.assertEqual((info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode)), (0, 0, 0o755))
        info = VOLUME.stat()
        self.assertEqual((info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode)), (10001, 10001, 0o700))
        for identity_file in (Path('/etc/passwd'), Path('/etc/group')):
            info = identity_file.stat()
            self.assertEqual((info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode)), (0, 0, 0o644))
        code = "import os,pathlib,pwd,grp; assert pwd.getpwuid(10001).pw_name=='supa-x'; assert grp.getgrgid(10001).gr_name=='supa-x'; p=pathlib.Path('/etc/supa-agent/x-worker/permission-proof'); p.write_text('synthetic'); p.chmod(0o600); assert os.getuid()==10001; assert not os.access('/etc/supa-agent/x-pilot',os.W_OK)"
        command(['/usr/bin/python3', '-c', code])
        self.assertEqual((VOLUME/'permission-proof').stat().st_uid, 10001)
        receipt = AUTH/'permission-proof.json'
        write(receipt, {'synthetic':True})
        self.assertEqual(stat.S_IMODE(receipt.stat().st_mode), 0o640)
        def unrelated_identity():
            os.setgroups([]); os.setgid(10002); os.setuid(10002)
            assert ctypes.CDLL(None).prctl(38, 1, 0, 0, 0) == 0
        denied = subprocess.run(['/usr/bin/python3','-c',"import os; assert not os.access('/etc/supa-agent/x-worker',os.X_OK); assert not os.access('/etc/supa-agent/x-pilot/permission-proof.json',os.R_OK)"], env=env(),cwd='/tmp',preexec_fn=unrelated_identity,capture_output=True,text=True,timeout=5)
        self.assertEqual(denied.returncode, 0, 'unrelated identity could access private fixture data')
        self.assertFalse(Path('/home').exists())
        self.assertFalse(Path('/run/credentials').exists())
        previous = os.umask(0o022)
        def mask():
            saved = os.umask(0)
            os.umask(saved)
            return saved
        try:
            with tempfile.TemporaryDirectory(dir='/tmp', prefix='private-report-') as folder:
                path = Path(folder)/'report.json'
                for caller_mask in (0o022, 0o077):
                    os.umask(caller_mask)
                    private_report(path, {'synthetic':True})
                    self.assertEqual(mask(), caller_mask)
                    self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
                    self.assertEqual(stat.S_IMODE(path.parent.stat().st_mode), 0o700)
                    with self.assertRaises(TypeError): private_report(path, object())
                    self.assertEqual(mask(), caller_mask)
                    self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
                    with self.assertRaises(OSError): private_report(path/'not-a-directory.json', {})
                    self.assertEqual(mask(), caller_mask)
        finally:
            os.umask(previous)


def setup_namespace(options):
    """A real namespace-owned root, not a host-root ownership exception.

    The chroot exposes only test binaries/SQL, PG distribution, Python runtime,
    required ELF dependencies, public locale data, proc/sysfs and null/random.
    No host home, dotenv, run directory, ledger or credential tree is mounted.
    """
    assert os.getuid() == 0
    subprocess.run(['mount', '--make-rprivate', '/'], check=True)
    subprocess.run(['mount', '-t', 'tmpfs', '-o', 'mode=755', 'tmpfs', '/tmp'], check=True)
    root = Path('/tmp/supa-pilot-root')
    fixture_directory(root)
    assert (root.stat().st_uid, root.stat().st_gid) == (0, 0)

    def destination(path):
        assert Path(path).is_absolute()
        result = root / str(path).lstrip('/')
        current = root
        for part in result.relative_to(root).parts[:-1]:
            current = fixture_directory(current/part)
        return result

    def readonly(source, target):
        target = destination(target)
        fixture_directory(target)
        subprocess.run(['mount', '--bind', str(source), str(target)], check=True)
        subprocess.run(['mount', '-o', 'remount,bind,ro,nosuid,nodev', str(target)], check=True)

    pg_source = Path(options.pg_root).resolve()
    assert (pg_source/'usr/lib/postgresql/18/bin/postgres').is_file()
    readonly(pg_source, PG_ROOT)
    stdlib = Path(sysconfig.get_path('stdlib')).resolve()
    readonly(stdlib, stdlib)
    for public_data in ('/usr/lib/locale', '/usr/share/zoneinfo'):
        if Path(public_data).is_dir(): readonly(Path(public_data), Path(public_data))

    # Copy only ELF dependency paths reported for these explicit runtime inputs.
    elf_inputs = [Path(options.binary).resolve(), Path(options.production_binary).resolve(),
                  Path(sys.executable).resolve(), Path('/bin/sh').resolve()]
    elf_inputs += [pg_source/'usr/lib/postgresql/18/bin'/name
                   for name in ('initdb', 'pg_ctl', 'postgres', 'psql', 'createdb', 'dropdb')]
    elf_inputs += list(stdlib.glob('lib-dynload/*.so'))
    library_env = {'PATH': '/usr/bin:/bin', 'LANG': 'C',
                   'LD_LIBRARY_PATH': str(pg_source/'usr/lib/x86_64-linux-gnu')}
    libraries = set()
    for binary in elf_inputs:
        found = subprocess.run(['ldd', str(binary)], env=library_env, capture_output=True,
                               text=True, timeout=10, check=True)
        for line in found.stdout.splitlines():
            fields = line.split()
            candidate = fields[2] if len(fields) >= 3 and fields[1] == '=>' else fields[0] if fields else ''
            if candidate.startswith('/'):
                path = Path(candidate)
                if not path.is_relative_to(pg_source):
                    assert str(path).startswith(('/lib/', '/lib64/', '/usr/lib/'))
                    libraries.add(path)
            assert 'not found' not in line, 'required fixture ELF dependency missing'
    for library in sorted(libraries):
        shutil.copy2(library, destination(library))
    shutil.copy2(sys.executable, destination(Path(sys.executable)))
    # Python subprocess helpers intentionally use /usr/bin/python3 when needed.
    if Path(sys.executable) != Path('/usr/bin/python3'):
        shutil.copy2(sys.executable, destination('/usr/bin/python3'))
    shutil.copy2('/bin/sh', destination('/bin/sh'))
    shutil.copy2(options.binary, destination(APP/'offline'))
    shutil.copy2(options.production_binary, destination(APP/'production'))
    fixture_directory(destination(APP/'sql'))
    for name in ('x-posting-foundation.sql', 'ecosystem-social.sql', 'social-review.sql',
                 'social-autonomy.sql', 'social-first-post.sql', 'x-worker-mock.sql', 'social-pilot-post.sql'):
        shutil.copy2(ROOT/'deployment'/name, destination(APP/'sql'/name))

    for directory in ('/etc', '/run', '/tmp', '/proc', '/sys', '/dev'):
        fixture_directory(destination(directory))
    for device in ('null', 'urandom', 'random'):
        target = destination('/dev/'+device); target.touch()
        subprocess.run(['mount', '--bind', '/dev/'+device, str(target)], check=True)
    subprocess.run(['mount', '--bind', '/proc', str(destination('/proc'))], check=True)
    subprocess.run(['mount', '-t', 'sysfs', '-o', 'ro,nosuid,nodev,noexec', 'sysfs', str(destination('/sys'))], check=True)
    subprocess.run(['ip', 'link', 'set', 'lo', 'up'], check=True)

    os.chroot(root)
    os.chdir('/')
    Path('/tmp').chmod(0o1777)
    Path('/etc/passwd').write_text('root:x:0:0:root:/tmp:/bin/false\nsupa-x:x:10001:10001:worker:/tmp:/bin/false\n')
    Path('/etc/group').write_text('root:x:0:\nsupa-x:x:10001:\n')
    for identity_file in (Path('/etc/passwd'), Path('/etc/group')):
        os.chown(identity_file, 0, 0)
        identity_file.chmod(0o644)
    for directory in (BASE, AUTH, AUTH/'approved', AUTH/'revoked'):
        fixture_directory(directory)
    fixture_directory(VOLUME, 0o700, 10001)
    for binary in (APP/'offline', APP/'production'): binary.chmod(0o755)
    trusted = [Path('/'), Path('/etc'), BASE, AUTH, AUTH/'approved', AUTH/'revoked']
    inventory = []
    for path in trusted:
        info = path.stat()
        assert (info.st_uid, info.st_gid) == (0, 0) and stat.S_IMODE(info.st_mode) == 0o755
        inventory.append({'path':str(path), 'uid':info.st_uid, 'gid':info.st_gid,
                          'mode':oct(info.st_mode & 0o777)})
    assert set(os.listdir('/sys/class/net')) == {'lo'}
    assert not Path('/home').exists() and not Path('/var').exists()
    assert list(Path('/run').iterdir()) == []
    print(json.dumps({'fixture_ownership':inventory, 'external_network':False,
                      'host_private_trees_exposed':False}), flush=True)


def compile_production_isolation(report_dir):
    """Expected build rejection, never an arbitrary compiler/dependency failure.

    Use the same build mode as the reviewed executable, not unrelated cached
    cargo-check metadata. No cleaning, network, retries or runtime execution.
    """
    report_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    build_env = {'PATH': os.environ['PATH'], 'HOME': os.environ['HOME'], 'LANG':'C.UTF-8',
                 'CARGO_TARGET_DIR': str(ROOT/'target/work-review-validation'),
                 'CARGO_BUILD_JOBS':'1', 'CARGO_INCREMENTAL':'0', 'CARGO_PROFILE_DEV_DEBUG':'0'}
    for name in ('RUSTUP_HOME', 'CARGO_HOME'):
        if name in os.environ: build_env[name] = os.environ[name]
    args = ['cargo','build','--offline','--locked','--message-format=json','-p','supa-runtime',
            '--bin','supa-x-pilot','--features','pilot-offline-tests']
    result = subprocess.run(args, cwd=ROOT, env=build_env, capture_output=True, text=True, timeout=180)
    messages = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    errors = [item for item in messages if item.get('reason')=='compiler-message'
              and item['message']['level']=='error']
    expected = 'offline validation is forbidden in the production pilot executable'
    passed = (result.returncode==101 and len(errors)==1
              and errors[0]['target']['name']=='supa-x-pilot'
              and errors[0]['target']['kind']==['bin']
              and errors[0]['message']['message']==expected
              and errors[0]['message']['code'] is None)
    # Diagnostic text only, never environment, credentials or command headers.
    evidence = {'command':args, 'exit_status':result.returncode, 'expected_rejection':True,
                'passed':passed, 'error_diagnostics':[{'target':item['target']['name'],
                'code':item['message']['code'], 'message':item['message']['message']} for item in errors],
                'automatic_retry':False, 'network':'cargo offline', 'production_requests':0}
    private_report(report_dir/'production-isolation.json', evidence)
    print(json.dumps(evidence))
    return 0 if passed else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary'); parser.add_argument('--production-binary')
    parser.add_argument('--compile-gate',action='store_true')
    parser.add_argument('--report-dir',type=Path,default=ROOT/'artifacts/x-pilot-packaged')
    parser.add_argument('--pg-root',default=str(ROOT/'artifacts/ecosystem-pg18-binaries/root'))
    parser.add_argument('--inside',action='store_true'); parser.add_argument('--focused',action='store_true')
    options = parser.parse_args()
    if options.compile_gate:
        if options.inside or options.focused: parser.error('compile gate must run outside runtime fixtures')
        raise SystemExit(compile_production_isolation(options.report_dir))
    if not options.binary or not options.production_binary:
        parser.error('runtime fixture requires both explicitly identified executables')
    if not options.inside:
        argv = [sys.executable, str(Path(__file__).resolve()), '--inside', '--binary', str(Path(options.binary).resolve()), '--production-binary',str(Path(options.production_binary).resolve()), '--pg-root',str(Path(options.pg_root).resolve())]
        if options.focused: argv.append('--focused')
        # Foreground only, no credentials/URLs in arguments or inherited env.
        result = subprocess.run(['unshare','--user','--map-root-user','--map-auto','--mount','--net','--pid','--fork','--mount-proc','--kill-child',*argv], env={'PATH':'/usr/sbin:/usr/bin:/sbin:/bin','LANG':'C.UTF-8'}, timeout=180)
        raise SystemExit(result.returncode)
    # Deliberately exercise fixture creation under the previously problematic mask.
    previous = os.umask(0o077) if options.focused else None
    try:
        setup_namespace(options)
    finally:
        if previous is not None: os.umask(previous)
    name = 'test_success_terminates_and_restart_does_not_replay'
    suite = unittest.TestSuite([Permissions('test_fixture_and_report_permissions'), Packaged('test_runtime_owned_receipt_is_denied'), Packaged('test_runtime_owned_receipt_ancestor_is_denied'), Packaged(name)]) if options.focused else unittest.TestSuite([unittest.defaultTestLoader.loadTestsFromTestCase(Permissions), unittest.defaultTestLoader.loadTestsFromTestCase(Packaged)])
    result = unittest.TextTestRunner(verbosity=2,failfast=True).run(suite)
    raise SystemExit(0 if result.wasSuccessful() else 1)


if __name__ == '__main__': main()
