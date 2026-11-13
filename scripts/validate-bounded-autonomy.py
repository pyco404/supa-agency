#!/usr/bin/env python3
"""One no-retry, socket-only PostgreSQL 18 synthetic autonomy test. No dotenv loading."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import uuid
from urllib.parse import urlencode

ROOT = Path(__file__).resolve().parents[1]
PGROOT = ROOT / 'artifacts/ecosystem-pg18-binaries/root'
PG = PGROOT / 'usr/lib/postgresql/18/bin'
OUT = ROOT / 'artifacts/phase5-bounded-autonomy/continuation-1'
WORKER = ROOT / 'target/work-review-validation/debug/supa-bounded-worker'


SQL_NULL = '__SUPA_SQL_NULL__'


def decode_sql_json(output):
    """SQL NULL is explicit; empty/malformed responses remain failures."""
    if output == SQL_NULL:
        return None
    return json.loads(output)


def decode_inventory(output):
    """Ignore JSON object key order, preserving every inventory field and value."""
    inventory = json.loads(output)
    if not isinstance(inventory, list) or any(not isinstance(item, dict) for item in inventory):
        raise ValueError('invalid_cluster_inventory')
    return inventory


def main(extra=None, setup_only=False):
    os.umask(0o077)
    OUT.mkdir(parents=True, exist_ok=True)
    attempt = OUT / 'integration-attempt.json'
    if attempt.exists():
        raise RuntimeError('attempt_already_consumed_no_automatic_retry')
    assert WORKER.is_file(), 'build_worker_before_integration'
    assert shutil.disk_usage('/tmp').free > 300 * 1024 * 1024
    work = Path(tempfile.mkdtemp(prefix='supa-bounded-test-'))
    socket = work / 'socket'
    socket.mkdir(mode=0o700)
    data = work / 'data'
    env = {k: os.environ[k] for k in ['PATH', 'HOME']}
    env.update(LANG='C.UTF-8', LC_ALL='C.UTF-8', TZ='UTC',
               LD_LIBRARY_PATH=str(PGROOT / 'usr/lib/x86_64-linux-gnu'),
               PGHOST=str(socket), PGPORT='55483', PGUSER='bounded_admin',
               PGDATABASE='postgres', PGPASSFILE='/dev/null', PGSERVICEFILE='/dev/null')
    registered = decode_inventory(subprocess.check_output(['pg_lsclusters', '--json']))
    migrations = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((ROOT / 'migrations').glob('*.sql'))}
    report = {'passed': False, 'attempts': 1, 'retries': 0, 'paid_requests': 0, 'railway_operations': 0, 'checks': []}
    attempt.write_text(json.dumps(report))
    started = False

    def run(args, input=None, timeout=60):
        return subprocess.run([str(a) for a in args], input=input, text=True, cwd=ROOT,
                              env=env, capture_output=True, timeout=timeout)

    def sql(query, role=None, deny=False, category=None):
        prefix = 'SET ROLE ' + role + ';' if role else ''
        r = run([PG / 'psql', '-X', '-qAt', '-P', 'null=' + SQL_NULL, '-v', 'ON_ERROR_STOP=1'], prefix + query)
        if deny:
            assert r.returncode != 0, 'expected_denial'
            if category is not None:
                assert category in r.stderr, 'unexpected_denial_category'
            return
        if r.returncode:
            (OUT / 'sql-failure.log').write_text(r.stderr[:4000])
            raise RuntimeError('disposable_sql_failed_see_sanitized_log')
        return r.stdout.strip()

    def value(query, role=None):
        return decode_sql_json(sql(query, role))

    def literal(v):
        return "'" + json.dumps(v).replace("'", "''") + "'::jsonb"

    def command(actor, agent, op, body=None, deny=False):
        return_query = f"SELECT agent_bounded.command('{actor}','{op}',{literal({'agent_id': agent, **(body or {})})});"
        if deny:
            sql(return_query, 'bounded_api', True)
            return
        return value(return_query, 'bounded_api')

    owner, other = str(uuid.uuid4()), str(uuid.uuid4())
    agents = []

    def fixture(actor):
        agent, goal = str(uuid.uuid4()), str(uuid.uuid4())
        config = {'request_id': str(uuid.uuid4()), 'name': 'Synthetic bounded agent', 'description': 'Disposable fixture only',
                  'purpose': 'Review synthetic incident notes', 'personality': {'voice': 'Concise and curious', 'traits': []},
                  'goals': ['Review synthetic incident notes'], 'requested_capabilities': ['read_memory', 'remember', 'evaluate_goal'],
                  'autonomy': 'disabled', 'spending_limit_microusd': 0, 'private_consent': True}
        candidate = {'id': agent, 'name': 'Synthetic bounded agent', 'lifecycle': 'paused',
                     'goals': [{'id': goal, 'description': 'Review synthetic incident notes', 'completed': False}],
                     'memory': [], 'capabilities': [], 'config': {'scheduler_enabled': False}}
        sql(f"INSERT INTO agents(id,data) VALUES('{agent}',{literal(candidate)});"
            f"INSERT INTO agent_access(agent_id,principal_id,role,granted_by) VALUES('{agent}','{actor}','owner','{actor}');"
            f"INSERT INTO agent_factory.submissions(creator_id,request_id,agent_id,configuration) VALUES('{actor}','{uuid.uuid4()}','{agent}',{literal(config)});")
        agents.append(agent)
        return agent, goal

    def scope(goal, cycles=3):
        return {'goal_id': goal, 'priority': 1, 'mode': 'deterministic',
                'input': {'task': 'Review synthetic incident notes', 'desired_outcome': 'Synthetic',
                          'material': 'Synthetic latency rose after a test change. Synthetic service recovered after restart. Causation is unknown.',
                          'synthetic_only': True, 'retain_memory': False},
                'max_cycles': cycles, 'max_actions': cycles * 3, 'max_spending_microusd': 0,
                'interval_seconds': 5, 'valid_for_seconds': 300}

    def control(actor, agent, op, extra=None, deny=False):
        r = command(actor, agent, 'get')['run']
        body = {'request_id': str(uuid.uuid4()), 'expected_revision': r['revision'],
                'scope_sha256': r['scope_sha256'], 'confirmed': True, **(extra or {})}
        result = command(actor, agent, op, body, deny)
        return result, body

    def activate(actor, agent, goal, cycles=3):
        command(actor, agent, 'create', {'request_id': str(uuid.uuid4()), 'scope': scope(goal, cycles)})
        control(actor, agent, 'approve')
        return control(actor, agent, 'start')[1]

    def once():
        worker_env = dict(env)
        worker_env.update(DATABASE_URL='postgresql://bounded_executor@localhost/postgres?' + urlencode({'host': str(socket), 'port': 55483}),
                          SUPA_BOUNDED_WORKER_ENABLED='true', SUPA_PROVIDER='offline')
        r = subprocess.run([str(WORKER), '--once'], env=worker_env, cwd=ROOT, capture_output=True, text=True, timeout=30)
        if r.returncode:
            (OUT / 'worker-failure.log').write_text(r.stdout[-2000:] + r.stderr[-2000:])
            raise RuntimeError('worker_failed_no_retry')
        return json.loads(r.stdout.splitlines()[-1])

    def restart():
        nonlocal started
        r = run([PG / 'pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop'])
        assert r.returncode == 0, 'postgres_stop_failed'
        started = False
        r = run([PG / 'pg_ctl', '-D', data, '-l', OUT / 'postgres.log', '-w', 'start'])
        assert r.returncode == 0, 'postgres_restart_failed'
        started = True

    try:
        assert 'PostgreSQL) 18.' in run([PG / 'postgres', '--version']).stdout
        r = run([PG / 'initdb', '-D', data, '-U', 'bounded_admin', '--auth-local=trust', '--auth-host=reject', '--wal-segsize=1', '--encoding=UTF8', '--locale=C.UTF-8', '--no-instructions'])
        assert r.returncode == 0, 'initdb_failed'
        with (data / 'postgresql.conf').open('a') as f:
            f.write(f"\nlisten_addresses=''\nunix_socket_directories='{socket}'\nport=55483\nmax_wal_size='32MB'\nmin_wal_size='2MB'\n")
        r = run([PG / 'pg_ctl', '-D', data, '-l', OUT / 'postgres.log', '-w', 'start'])
        assert r.returncode == 0, 'postgres_start_failed'
        started = True
        for p in sorted((ROOT / 'migrations').glob('*.sql')):
            sql(p.read_text())
        for name in ['ecosystem-social', 'agent-factory', 'agent-supervised', 'agent-bounded']:
            sql((ROOT / 'deployment' / (name + '.sql')).read_text())
        sql("REVOKE CREATE ON SCHEMA public FROM PUBLIC;"
            "CREATE ROLE bounded_api LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;"
            "CREATE ROLE bounded_executor LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;"
            "GRANT USAGE ON SCHEMA agent_bounded TO bounded_api,bounded_executor;"
            "GRANT EXECUTE ON FUNCTION agent_bounded.command(uuid,text,jsonb) TO bounded_api;"
            "GRANT EXECUTE ON FUNCTION agent_bounded.claim(),agent_bounded.advance(uuid,uuid,integer,jsonb) TO bounded_executor;")
        if setup_only:
            assert extra is not None, 'setup_only_requires_assertions'
            extra(locals())
            report['passed'] = True
            return
        a, g = fixture(owner)
        b, bg = fixture(other)
        baseline = sql("SELECT encode(sha256(convert_to(jsonb_agg(to_jsonb(a) ORDER BY id)::text,'UTF8')),'hex') FROM agents a;")
        baseline_ledger = sql('SELECT count(*) FROM simulated_ledger;')
        command(other, a, 'get', deny=True)
        for change in [{'max_spending_microusd': 1}, {'max_cycles': 11}, {'max_actions': 8}, {'interval_seconds': 4}, {'mode': 'openai'}, {'goal_id': bg}]:
            command(owner, a, 'create', {'request_id': str(uuid.uuid4()), 'scope': {**scope(g), **change}}, deny=True)
        assert not once()['cycle_completed'], 'no_unapproved_execution'
        start_body = activate(owner, a, g)
        assert once()['cycle_completed']
        r = command(owner, a, 'get')['run']
        assert r['cycles_started'] == 1 and r['actions_reserved'] == 3
        assert len(r['cycles']) == 1 and r['cycles'][0]['state'] == 'awaiting_review'
        assert r['cycles'][0]['approved_memory'] is None
        assert not once()['cycle_completed'], 'minimum_interval_not_enforced'
        control(owner, a, 'pause')
        assert command(owner, a, 'start', start_body)['run']['state'] == 'paused', 'start_replay_reactivated'
        restart()
        assert command(owner, a, 'get')['run']['state'] == 'paused'
        assert not once()['cycle_completed']
        control(owner, a, 'resume')
        sql("SELECT pg_sleep(5);")
        assert once()['cycle_completed']
        sql("SELECT pg_sleep(5);")
        assert once()['cycle_completed']
        r = command(owner, a, 'get')['run']
        assert r['state'] == 'exhausted' and r['cycles_started'] == 3 and r['actions_reserved'] == 9
        control(owner, a, 'resume', deny=True)
        assert not once()['cycle_completed']
        cycle = r['cycles'][0]
        control(owner, a, 'review', {'task_id': cycle['task_id'], 'result_sha256': '0' * 64, 'accept': True, 'retain_memory': True}, deny=True)
        reviewed, review_body = control(owner, a, 'review', {'task_id': cycle['task_id'], 'result_sha256': cycle['result_sha256'], 'accept': True, 'retain_memory': True})
        assert reviewed['run']['cycles'][0]['approved_memory']['kind'] == 'hypothesis'
        assert command(owner, a, 'review', review_body) == reviewed
        report['checks'].extend(['two_owner_isolation', 'policy_bounds', 'three_genuine_deterministic_cycles', 'exact_scope_approval', 'three_actions_per_cycle', 'zero_spending', 'pause_resume', 'postgres_restart_persistence', 'no_start_replay', 'cycle_ceiling', 'review_hash', 'explicit_private_memory', 'idempotent_review'])
        activate(other, b, bg, 1)
        claim = value('SELECT agent_bounded.claim();', 'bounded_executor')
        assert claim is not None
        assert value('SELECT agent_bounded.claim();', 'bounded_executor') is None, 'duplicate_claim'
        before = command(other, b, 'get')['run']['cycles_started']
        sql(f"UPDATE agent_bounded.runs SET lease_until=clock_timestamp()-interval '1 second' WHERE agent_id='{b}';")
        assert value('SELECT agent_bounded.claim();', 'bounded_executor') is None
        assert command(other, b, 'get')['run']['state'] == 'quarantined'
        restart()
        assert not once()['cycle_completed']
        assert command(other, b, 'get')['run']['cycles_started'] == before
        c, cg = fixture(owner)
        activate(owner, c, cg, 1)
        sql(f"UPDATE agent_bounded.runs SET approved_until=clock_timestamp()-interval '1 second' WHERE agent_id='{c}';")
        assert not once()['cycle_completed']
        assert command(owner, c, 'get')['run']['state'] == 'exhausted'
        d, dg = fixture(owner)
        activate(owner, d, dg, 1)
        cl = value('SELECT agent_bounded.claim();', 'bounded_executor')
        control(owner, d, 'pause')
        sql(f"SELECT agent_bounded.advance('{cl['run_id']}','{cl['lease_id']}',{cl['task']['revision']},'{{}}');", 'bounded_executor', True)
        control(owner, d, 'stop')
        control(owner, d, 'resume', deny=True)
        report['checks'].extend(['duplicate_worker_exclusion', 'uncertain_lease_quarantine_no_replay', 'authorization_expiry', 'inflight_pause_fencing', 'permanent_stop'])
        # Protected sources and accounting were never mutated by coordinator execution.
        assert baseline == sql("SELECT encode(sha256(convert_to(jsonb_agg(to_jsonb(a) ORDER BY id)::text,'UTF8')),'hex') FROM agents a WHERE id IN ('" + a + "','" + b + "');")
        assert baseline_ledger == sql('SELECT count(*) FROM simulated_ledger;')
        assert sql("SELECT count(*) FROM agent_bounded.events e WHERE previous_sha256 IS DISTINCT FROM coalesce((SELECT sha256 FROM agent_bounded.events p WHERE p.run_id=e.run_id AND p.id<e.id ORDER BY id DESC LIMIT 1),repeat('0',64));") == '0'
        for role, query in [('bounded_api','SELECT agent_bounded.claim();'), ('bounded_executor',f"SELECT agent_bounded.command('{owner}','get',{literal({'agent_id': a})});"), ('bounded_api','SELECT * FROM agents;'), ('bounded_executor','UPDATE agents SET data=data;')]:
            sql(query, role, True)
        report['checks'].extend(['frozen_agent_integrity', 'accounting_unchanged', 'hash_linked_audit_integrity', 'separate_api_executor_grants'])
        if extra is not None:
            extra(locals())
        report['passed'] = True
    finally:
        if started or (data / 'postmaster.pid').exists():
            r = run([PG / 'pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop'])
            report['disposable_postgres_stopped'] = r.returncode == 0
        assert not (data / 'postmaster.pid').exists()
        assert registered == decode_inventory(subprocess.check_output(['pg_lsclusters', '--json']))
        assert migrations == {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((ROOT / 'migrations').glob('*.sql'))}
        report['fixture_bytes'] = sum(p.stat().st_size for p in work.rglob('*') if p.is_file())
        shutil.rmtree(work)  # Only the newly created stopped fixture, never existing resources.
        report['fixture_removed'] = not work.exists()
        attempt.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report), flush=True)


if __name__ == '__main__':
    main()
