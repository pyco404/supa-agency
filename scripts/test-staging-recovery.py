#!/usr/bin/env python3
"""Opt-in synthetic PG18 + OpenPGP rehearsal. No network, live URLs or real keys.

Two new socket-only clusters and disposable fixture keys only. No production
backup CLI. Uses vendored PG18 binaries; never calls registered-cluster tools to
start/stop servers. Only a sanitized result is retained in ignored artifacts.
"""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import zlib
import tarfile
import tempfile
import uuid

ROOT = Path(__file__).resolve().parents[1]
PG = ROOT / 'artifacts/ecosystem-pg18-binaries/root/usr/lib/postgresql/18/bin'
ADMIN = 'supa_recovery_fixture_admin'
RUNTIME = 'supa_recovery_fixture_runtime'


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--synthetic-only', action='store_true', required=True)
    parser.parse_args()
    os.umask(0o077)
    crypto = load('recovery_crypto', 'backup-crypto.py')
    fixtures = load('recovery_fixtures', 'test-backup-crypto.py').BackupCryptoTests
    fingerprint = load('recovery_fingerprint', 'persistent-postgres.py')
    before = json.loads(subprocess.check_output(['pg_lsclusters', '--json']))
    migrations = {p.name: hashlib.sha384(p.read_bytes()).hexdigest()
                  for p in sorted((ROOT / 'migrations').glob('*.sql'))}
    assert len(migrations) == 13
    work = Path(tempfile.mkdtemp(prefix='supa-recovery-test-', dir='/tmp'))
    ram = Path('/dev/shm') / work.name
    ram.mkdir(mode=0o700)
    clusters = []
    checks = {}

    def command(args, env, data=None):
        result = subprocess.run([str(a) for a in args], input=data, env=env,
                                capture_output=True, timeout=90)
        if result.returncode:
            # No raw stderr/SQL, environment, key material or rows.
            raise RuntimeError('synthetic_recovery_operation_failed_' + Path(args[0]).name)
        return result.stdout

    def make_cluster(index):
        socket = work / ('socket' + str(index)); socket.mkdir(mode=0o700)
        folder = ram / ('cluster' + str(index)); folder.mkdir(mode=0o700)
        env = {'PATH': '/usr/bin:/bin', 'HOME': str(work), 'LANG': 'C.UTF-8',
               'LC_ALL': 'C.UTF-8', 'PGHOST': str(socket), 'PGPORT': '55483',
               'PGUSER': ADMIN, 'PGDATABASE': 'postgres', 'PGPASSFILE': '/dev/null',
               'PGSERVICEFILE': '/dev/null', 'PGCONNECT_TIMEOUT': '5',
               'LD_LIBRARY_PATH': str(PG.parents[3] / 'lib/x86_64-linux-gnu')}
        data = folder / 'data'
        c = {'data': data, 'folder': folder, 'env': env, 'database': 'supa_recovery_fixture', 'running': False}
        clusters.append(c)
        command([PG / 'initdb', '-D', data, '-U', ADMIN, '--auth-local=trust',
                 '--auth-host=reject', '--wal-segsize=1', '--encoding=UTF8', '--locale=C.UTF-8', '--data-checksums'], env)
        with (data / 'postgresql.conf').open('a') as f:
            f.write(f"\nlisten_addresses=''\nunix_socket_directories='{socket}'\nunix_socket_permissions=0700\nport=55483\nmax_wal_size='32MB'\nmin_wal_size='2MB'\n")
        start(c)
        command([PG / 'createdb', '--template=template0', c['database']], env)
        assert json.loads(sql(c, "SELECT json_build_array(current_database(),current_user,current_setting('listen_addresses'),inet_server_addr(),current_setting('data_directory'))")) == [c['database'], ADMIN, '', None, str(data)]
        sql(c, f'CREATE ROLE {RUNTIME} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS')
        return c

    def start(c):
        command([PG / 'pg_ctl', '-D', c['data'], '-l', c['folder'] / 'postgres.log', '-w', 'start'], c['env'])
        c['running'] = True

    def stop(c):
        if c['running'] or (c['data'] / 'postmaster.pid').exists():
            command([PG / 'pg_ctl', '-D', c['data'], '-m', 'fast', '-w', 'stop'], c['env'])
        c['running'] = False

    def sql(c, statement, user=ADMIN):
        env = {**c['env'], 'PGDATABASE': c['database'], 'PGUSER': user}
        return command([PG / 'psql', '-XqAt', '--no-password', '-v', 'ON_ERROR_STOP=1'], env, statement.encode()).decode().strip()

    def state(c):
        trace = []
        def query(statement, database):
            output = sql(c, statement)
            trace.append((statement, output))
            return output
        fingerprint.sql = query
        digest = fingerprint.fingerprint(c['database'])
        c['fingerprint_trace'] = trace
        return digest

    # ACL arrays have no meaningful order. Normalize each privilege/grantor,
    # including implicit owner/PUBLIC defaults, without dropping any grants.
    acl_query = """WITH objects AS (
      SELECT 'schema:'||nspname AS identity,nspowner AS owner,nspacl AS acl,'n'::char AS kind,
             NULL::boolean AS definer,NULL::text[] AS settings FROM pg_namespace
       WHERE nspname IN ('public','ecosystem_social','agent_factory','agent_avatars')
      UNION ALL SELECT 'relation:'||n.nspname||'.'||c.relname,c.relowner,c.relacl,
        CASE WHEN c.relkind='S' THEN 's' ELSE 'r' END::char,NULL,NULL
       FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
       WHERE n.nspname IN ('public','ecosystem_social','agent_factory','agent_avatars')
         AND c.relkind IN ('r','p','v','m','f','S')
      UNION ALL SELECT 'function:'||p.oid::regprocedure::text,p.proowner,p.proacl,'f'::char,p.prosecdef,p.proconfig
       FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
       WHERE n.nspname IN ('ecosystem_social','agent_factory','agent_avatars'))
      SELECT json_agg(json_build_object('identity',o.identity,'owner',pg_get_userbyid(o.owner),
        'definer',o.definer,'settings',o.settings,'grants',
        (SELECT json_agg(json_build_array(pg_get_userbyid(x.grantor),
          CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.grantee) END,
          x.privilege_type,x.is_grantable) ORDER BY x.grantor::regrole::text,
          CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE x.grantee::regrole::text END,x.privilege_type,x.is_grantable)
         FROM aclexplode(coalesce(o.acl,acldefault(o.kind::"char",o.owner))) x))
        ORDER BY o.identity) FROM objects o"""

    try:
        source = make_cluster(0)
        version = sql(source, 'SHOW server_version'); assert version.startswith('18.')
        for path in sorted((ROOT / 'migrations').glob('*.sql')):
            sql(source, 'BEGIN;\n' + path.read_text() + '\nCOMMIT;')
        for filename in ('ecosystem-social.sql', 'agent-factory.sql', 'agent-avatars.sql'):
            sql(source, (ROOT / 'deployment' / filename).read_text())
        sql(source, f'''REVOKE CREATE ON SCHEMA public FROM PUBLIC;
        GRANT USAGE ON SCHEMA public,agent_factory,agent_avatars TO {RUNTIME};
        GRANT SELECT ON application_epoch,agent_access TO {RUNTIME};
        GRANT EXECUTE ON FUNCTION agent_factory.create_private(uuid,uuid,jsonb,jsonb,jsonb,jsonb),agent_factory.owned(uuid),agent_factory.managed(uuid),agent_avatars.command(uuid,text,jsonb),agent_avatars.create_agent(uuid,uuid,jsonb,jsonb,jsonb,jsonb,uuid) TO {RUNTIME};''')
        actor, request, agent, purpose, consent = [str(uuid.uuid4()) for _ in range(5)]
        config = {'request_id': request, 'name': 'Synthetic recovery fixture', 'purpose': 'Verify disposable restore only', 'autonomy': 'disabled', 'spending_limit_microusd': 0, 'private_consent': True}
        candidate = {'name': config['name'], 'id': agent, 'lifecycle': 'paused', 'capabilities': [], 'memory': [], 'config': {'scheduler_enabled': False, 'inference_cost_microusd': 0, 'operating_cost_microusd': 0, 'draft_cost_microusd': 0}}
        purpose_body = {'id': purpose, 'agent_id': agent, 'creator_id': actor, 'objective': config['purpose'], 'revision': 1, 'public_summary': None}
        consent_body = {'request_id': consent, 'public': False, 'revoked': True, 'categories': []}
        def literal(value): return "'" + json.dumps(value).replace("'", "''") + "'::jsonb"
        call = f"SELECT agent_factory.create_private('{actor}','{request}'," + ','.join(literal(x) for x in (config,candidate,purpose_body,consent_body)) + ')'
        assert sql(source, call, RUNTIME) == agent
        # Valid normalized PNG fixture; no external image or private metadata.
        def png_chunk(kind, raw):
            return struct.pack('!I', len(raw)) + kind + raw + struct.pack('!I', zlib.crc32(kind + raw))
        png = (b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('!IIBBBBB',512,512,8,6,0,0,0))
               + png_chunk(b'IDAT', zlib.compress((b'\0' + b'\x60\x68\x40\xff' * 512) * 512)) + png_chunk(b'IEND',b''))
        asset = str(uuid.uuid4()); image_sha = hashlib.sha256(png).hexdigest()
        sql(source, f"INSERT INTO agent_avatars.assets(id,owner_id,request_id,kind,png,sha256) VALUES('{asset}','{actor}','{uuid.uuid4()}','default',decode('{png.hex()}','hex'),'{image_sha}'); INSERT INTO agent_avatars.revisions(agent_id,revision,asset_id,actor) VALUES('{agent}',1,'{asset}','{actor}')")
        expected = state(source); acl = json.loads(sql(source, acl_query))
        dump = command([PG / 'pg_dump', '--format=custom', '--no-owner', source['database']], source['env'])
        assert state(source) == expected
        inventory = {'format': 1, 'synthetic_only': True, 'postgres_major': 18,
                     'dump_sha256': hashlib.sha256(dump).hexdigest(), 'state_sha256': expected,
                     'migration_source_sha384': migrations, 'runtime_role': RUNTIME,
                     'runtime_attributes': ['NOSUPERUSER','NOCREATEDB','NOCREATEROLE','NOINHERIT','NOBYPASSRLS']}
        bundle = io.BytesIO()
        with tarfile.open(fileobj=bundle, mode='w') as tar:
            for name, raw in [('database.dump', dump), ('inventory.json', crypto.canonical(inventory))]:
                entry = tarfile.TarInfo(name); entry.size = len(raw); entry.mode = 0o600
                tar.addfile(entry, io.BytesIO(raw))
        # Real keys below are disposable test keys from the existing synthetic suite.
        fixtures.setUpClass()
        cipher = crypto.encrypt_bytes(fixtures.public, bundle.getvalue(), fixtures.recipient_pin, fixtures.subkey)
        snapshot = str(uuid.uuid4())
        manifest = crypto.manifest(cipher, snapshot, fixtures.recipient_pin, fixtures.subkey, fixtures.signer_pin, fixtures.signer_key)
        signature = fixtures.sign(manifest)
        for bad_cipher in [cipher[:-16], bytes([cipher[0] ^ 1]) + cipher[1:]]:
            try:
                crypto.verify_snapshot(fixtures.verifier_home, manifest, signature, bad_cipher, snapshot, fixtures.recipient_pin, fixtures.subkey, fixtures.signer_pin, fixtures.signer_key)
            except ValueError: pass
            else: raise AssertionError('tampered_ciphertext_accepted')
        checks['tampered_ciphertext_rejected_before_decryption'] = True
        fresh_key = fixtures.home('independent-fixture-key'); fresh_public = fixtures.home('independent-fixture-verifier')
        # Fixture-only recovery export stays in memory and is never logged or retained.
        secret = fixtures.cmd(fixtures.recipient, ['--export-secret-keys', fixtures.recipient_pin]).stdout
        fixtures.cmd(fresh_key, ['--import'], secret); del secret
        fixtures.cmd(fresh_public, ['--import'], fixtures.signer_export)
        stop(source)
        checked = crypto.verify_snapshot(fresh_public, manifest, signature, cipher, snapshot, fixtures.recipient_pin, fixtures.subkey, fixtures.signer_pin, fixtures.signer_key)
        assert checked['verified'] is True
        decrypted = fixtures.cmd(fresh_key, ['--status-fd','2','--decrypt'], cipher)
        assert b'[GNUPG:] DECRYPTION_INFO 0 9 2' in decrypted.stderr and b'[GNUPG:] DECRYPTION_OKAY' in decrypted.stderr
        # No decryption pipeline into pg_restore; authenticate fully before use.
        with tarfile.open(fileobj=io.BytesIO(decrypted.stdout)) as tar:
            members = tar.getmembers()
            assert sorted(m.name for m in members) == ['database.dump','inventory.json'] and all(m.isfile() and m.size <= crypto.MAX_BYTES for m in members)
            restored_dump = tar.extractfile('database.dump').read()
            recovered_inventory = json.load(tar.extractfile('inventory.json'))
        assert recovered_inventory == inventory and hashlib.sha256(restored_dump).hexdigest() == inventory['dump_sha256']
        destination = make_cluster(1)
        archive = ram / 'verified-synthetic.dump'; archive.write_bytes(restored_dump)
        command([PG / 'pg_restore', '--exit-on-error', '--single-transaction', '--no-owner', '--dbname', destination['database'], archive], destination['env'])
        actual = state(destination)
        if actual != expected:
            for i, ((query, left), (_, right)) in enumerate(zip(source['fingerprint_trace'], destination['fingerprint_trace'])):
                if left != right:
                    lrows, rrows = left.splitlines(), right.splitlines()
                    print(json.dumps({'synthetic_comparison_mismatch': i, 'different_statement_indexes': [j for j, (x,y) in enumerate(zip(lrows,rrows)) if x != y], 'source_lines': len(lrows), 'restore_lines': len(rrows)}))
            raise AssertionError('synthetic_fingerprint_mismatch')
        if json.loads(sql(destination, acl_query)) != acl:
            raise AssertionError('synthetic_acl_mismatch')
        sql(destination, f'GRANT UPDATE ON public.agents TO {RUNTIME}')
        assert json.loads(sql(destination, acl_query)) != acl
        sql(destination, f'REVOKE UPDATE ON public.agents FROM {RUNTIME}')
        assert json.loads(sql(destination, acl_query)) == acl
        assert sql(destination, f"SELECT sha256 FROM agent_avatars.assets WHERE id='{asset}'") == image_sha
        assert sql(destination, f"SELECT count(*) FROM agent_factory.owned('{actor}')", RUNTIME) == '1'
        assert sql(destination, f"SELECT count(*) FROM agent_factory.owned('{uuid.uuid4()}')", RUNTIME) == '0'
        assert sql(destination, "SELECT has_table_privilege(current_user,'public.agents','INSERT,UPDATE,DELETE')", RUNTIME) == 'f'
        try: sql(destination, 'UPDATE public.agents SET simulated_balance_microusd=1', RUNTIME)
        except RuntimeError: pass
        else: raise AssertionError('unauthorized_write_accepted')
        assert sql(destination, call, RUNTIME) == agent
        assert state(destination) == expected
        stop(destination); start(destination)
        assert state(destination) == expected
        checks.update(signature_and_sha256_verified=True, aes256_ocb_decryption_verified=True,
                      independent_fixture_key_home=True, source_stopped_before_restore=True,
                      full_schema_data_constraints_indexes_sequences_match=True, restricted_acls_and_functions_match=True, extra_grant_detected=True, avatar_bytes_restored=True,
                      owner_isolation_and_duplicate_prevention=True, direct_runtime_write_denied=True,
                      restored_restart_persistence=True, migration_sources_unchanged=True)
        assert migrations == {p.name: hashlib.sha384(p.read_bytes()).hexdigest() for p in sorted((ROOT/'migrations').glob('*.sql'))}
        result = {'passed': True, 'synthetic_only': True, 'postgres_version': version,
                  'migration_sql_files': 13, 'feature_extensions': 3, 'checks': checks,
                  'external_requests': 0, 'live_database_exported': False, 'production_keys_created': False,
                  'actual_independent_storage_tested': False, 'railway_recovery_verified': False,
                  'limitation': 'Two local disposable clusters and synthetic keys; no independent machine, storage or actual Railway backup was tested.'}
        out = ROOT / 'artifacts/staging-recovery-preparation'; out.mkdir(mode=0o700, exist_ok=True)
        (out / 'synthetic-result.json').write_text(json.dumps(result, indent=2)+'\n')
        print(json.dumps(result))
    finally:
        for c in reversed(clusters): stop(c)
        fixtures.doClassCleanups()
        assert before == json.loads(subprocess.check_output(['pg_lsclusters','--json']))
        assert all(not (c['data']/'postmaster.pid').exists() for c in clusters)
        shutil.rmtree(work); shutil.rmtree(ram)


if __name__ == '__main__':
    main()
