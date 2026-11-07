#!/usr/bin/env python3
"""Disposable socket-only PG18 regression for the exact staging export grants."""
import datetime
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid

ROOT = Path(__file__).resolve().parents[1]
PG = ROOT / 'artifacts/ecosystem-pg18-binaries/root/usr/lib/postgresql/18/bin'
spec = importlib.util.spec_from_file_location('export_role', ROOT / 'scripts/recovery-export-role.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


def main():
    os.umask(0o077)
    work = Path(tempfile.mkdtemp(prefix='supa-export-role-test-', dir='/dev/shm'))
    data, socket = work / 'data', work / 'socket'
    socket.mkdir(mode=0o700)
    env = {'PATH': '/usr/bin:/bin', 'HOME': str(work), 'LANG': 'C.UTF-8',
           'LD_LIBRARY_PATH': str(PG.parents[3] / 'lib/x86_64-linux-gnu'),
           'PGHOST': str(socket), 'PGPORT': '55491', 'PGUSER': 'export_test_admin',
           'PGDATABASE': 'postgres', 'PGPASSFILE': '/dev/null', 'PGSERVICEFILE': '/dev/null'}
    running = False

    def command(binary, args, text=None, user=None, allow_failure=False):
        result = subprocess.run([str(PG / binary), *args], input=text, text=True,
                                env={**env, **({'PGUSER': user} if user else {})},
                                capture_output=True, timeout=60)
        if result.returncode and not allow_failure:
            raise RuntimeError('disposable_export_role_test_failed_' + binary + '_' + result.stderr[-120:])
        return result

    def sql(text, user=None, allow_failure=False):
        return command('psql', ['-XqAt', '--no-password', '-v', 'ON_ERROR_STOP=1',
                               '-v', 'VERBOSITY=sqlstate'], text, user, allow_failure)

    try:
        command('initdb', ['-D', str(data), '-U', env['PGUSER'], '--auth-local=trust',
                           '--auth-host=reject', '--wal-segsize=1', '--locale=C.UTF-8'])
        with (data / 'postgresql.conf').open('a') as f:
            f.write(f"\nlisten_addresses=''\nunix_socket_directories='{socket}'\nport=55491\nmax_wal_size='32MB'\nmin_wal_size='2MB'\n")
        command('pg_ctl', ['-D', str(data), '-l', str(work / 'server.log'), '-w', 'start'])
        running = True
        command('createdb', ['--template=template0', 'supa_staging'])
        env['PGDATABASE'] = 'supa_staging'
        for path in sorted((ROOT / 'migrations').glob('*.sql')):
            sql('BEGIN;\n' + path.read_text() + '\nCOMMIT;')
        sql('CREATE TABLE public._sqlx_migrations(version bigint PRIMARY KEY,checksum bytea NOT NULL,success boolean NOT NULL); REVOKE CREATE ON SCHEMA public FROM PUBLIC; REVOKE TEMPORARY ON DATABASE supa_staging FROM PUBLIC;')
        relations = json.loads(sql("SELECT json_agg(json_build_object('name',n.nspname||'.'||c.relname,'kind',c.relkind) ORDER BY n.nspname,c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('public','epoch_guard') AND c.relkind IN ('r','p','S')").stdout)
        tables = [x['name'] for x in relations if x['kind'] != 'S']
        sequences = [x['name'] for x in relations if x['kind'] == 'S']
        schemas = ['public', 'epoch_guard']
        inventory_spec = importlib.util.spec_from_file_location('logical_inventory', ROOT / 'scripts/recovery-logical-inventory.py')
        inventory = importlib.util.module_from_spec(inventory_spec)
        inventory_spec.loader.exec_module(inventory)
        baseline = inventory.stream(sql('SET search_path=pg_catalog; ' + inventory.sql(tables)).stdout)
        group = 'supa_recovery_export_' + uuid.uuid4().hex[:12]
        login = group.replace('_export_', '_login_')
        expiry = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(hours=1)).replace(microsecond=0).isoformat()
        create = m.create_sql(group, login, 'SyntheticFixtureOnly_' + 'x' * 40, expiry, tables, sequences, schemas)
        sql(create)
        restricted = inventory.stream(sql(f'SET ROLE {m.ident(group)}; SET search_path=pg_catalog; ' + inventory.sql(tables, (group, login)), login).stdout)
        assert restricted == baseline, 'restricted_inventory_incomplete'
        effective = sql(f'SET ROLE {m.ident(group)}; SELECT current_user; SELECT count(*) FROM public.agents;', login).stdout.splitlines()
        assert effective == [group, '0']
        for table in tables + sequences:
            sql(f'SET ROLE {m.ident(group)}; SELECT * FROM {".".join(m.ident(x) for x in table.split("."))} LIMIT 0;', login)
        denied = [
            'UPDATE public.agents SET simulated_balance_microusd=0 WHERE false;',
            'DELETE FROM public.financial_audit WHERE false;',
            'CREATE TABLE public.supa_export_forbidden(id integer);',
            'CREATE SCHEMA supa_export_forbidden;',
            'CREATE TEMP TABLE supa_export_forbidden(id integer);',
            f'GRANT {m.ident(group)} TO export_test_admin;',
            'SET ROLE export_test_admin;',
            "SELECT epoch_guard.approve_operation('00000000-0000-4000-8000-000000000001','{}'::jsonb);",
        ]
        for statement in denied:
            result = sql(f'BEGIN READ WRITE; SET LOCAL ROLE {m.ident(group)}; {statement} ROLLBACK;', login, True)
            assert result.returncode == 3 and '42501' in result.stderr, 'expected_privilege_denial'
        sequence_writes = sql(f"SET ROLE {m.ident(group)}; SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('public','epoch_guard') AND c.relkind='S' AND has_sequence_privilege(current_user,c.oid,'USAGE,UPDATE');", login).stdout.strip()
        assert sequence_writes == '0'
        sql(m.cleanup_sql(group, login, tables, sequences, schemas))
        assert inventory.stream(sql('SET search_path=pg_catalog; ' + inventory.sql(tables)).stdout) == baseline, 'original_acl_or_schema_changed'
        assert sql(f"SELECT count(*) FROM pg_roles WHERE rolname IN ({m.literal(group)},{m.literal(login)});").stdout.strip() == '0'
        for bad_tables in (tables[:-1], tables + [tables[0]]):
            try:
                m.inventory(bad_tables, sequences, schemas)
            except ValueError:
                pass
            else:
                raise AssertionError('invalid_inventory_accepted')
        print(json.dumps({'passed': True, 'postgres': sql('SHOW server_version').stdout.strip(),
                          'tables': len(tables), 'sequences': len(sequences),
                          'permission_denials': len(denied), 'sequence_write_privileges': 0,
                          'temporary_roles_removed': True, 'live_connections': 0}))
    finally:
        if running:
            command('pg_ctl', ['-D', str(data), '-m', 'fast', '-w', 'stop'])
        shutil.rmtree(work)


if __name__ == '__main__':
    main()
