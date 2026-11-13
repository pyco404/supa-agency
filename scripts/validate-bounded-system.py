#!/usr/bin/env python3
"""Extend the disposable Phase 5 suite with real processes/browser and fault boundaries."""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import runpy
import signal
import socket
import subprocess
import time
import urllib.error
import urllib.request
import uuid
from urllib.parse import urlencode

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'artifacts/phase5-bounded-autonomy/system'
M = runpy.run_path(str(ROOT / 'scripts/validate-bounded-autonomy.py'))


def extend(f):
    sql, value, literal, command, control, activate, fixture, once = (f[k] for k in ['sql', 'value', 'literal', 'command', 'control', 'activate', 'fixture', 'once'])
    report, env, owner, other = (f[k] for k in ['report', 'env', 'owner', 'other'])
    children = []

    def port():
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            return s.getsockname()[1]

    def start(binary, environment, name):
        log = (OUT / (name + '.log')).open('ab')
        child = subprocess.Popen([str(binary)], cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        children.append((child, log))
        return child

    def stop(child):
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=5)
                raise RuntimeError('child_shutdown_deadline')
        assert child.returncode == 0, 'child_shutdown_failed'

    def http(origin, path, token=None):
        req = urllib.request.Request(origin + path, headers={'Authorization': 'Bearer ' + token} if token else {})
        try:
            with urllib.request.urlopen(req, timeout=3) as response:
                return response.status, json.load(response)
        except urllib.error.HTTPError as e:
            e.close()
            return e.code, None

    def ready(child, origin, path):
        end = time.monotonic() + 20
        while time.monotonic() < end:
            if child.poll() is not None:
                raise RuntimeError('process_exited_before_readiness')
            try:
                status, result = http(origin, path)
                assert status == 200, 'unhealthy_readiness'
                return result
            except urllib.error.URLError:
                time.sleep(0.1)
        raise RuntimeError('process_readiness_deadline')

    def resources(child):
        status = Path(f'/proc/{child.pid}/status').read_text()
        fields = {line.split(':', 1)[0]: line.split(':', 1)[1].strip() for line in status.splitlines() if ':' in line}
        stat = Path(f'/proc/{child.pid}/stat').read_text().split()
        return {'rss_kib': int(fields['VmRSS'].split()[0]), 'peak_rss_kib': int(fields['VmHWM'].split()[0]), 'cpu_seconds': round((int(stat[13]) + int(stat[14])) / os.sysconf('SC_CLK_TCK'), 3)}

    api = ROOT / 'target/work-review-validation/debug/supa-api'
    assert api.is_file(), 'build_api_before_system_test'
    try:
        e, eg = fixture(owner)
        activate(owner, e, eg, 1)
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            results = list(executor.map(lambda _: once(), range(2)))
        assert sum(r['cycle_completed'] for r in results) == 1, 'concurrent_duplicate_cycle'
        revoked, rg = fixture(owner)
        activate(owner, revoked, rg, 1)
        sql(f"DELETE FROM agent_access WHERE agent_id='{revoked}' AND principal_id='{owner}';")
        assert not once()['cycle_completed']
        assert sql(f"SELECT state FROM agent_bounded.runs WHERE agent_id='{revoked}';") == 'quarantined'
        command(owner, revoked, 'get', deny=True)
        guarded, gg = fixture(owner)
        command(owner, guarded, 'create', {'request_id': str(uuid.uuid4()), 'scope': f['scope'](gg, 1)})
        control(owner, guarded, 'start', deny=True)
        control(owner, guarded, 'approve', {'scope_sha256': '0' * 64}, deny=True)
        control(owner, guarded, 'approve')
        control(owner, guarded, 'start')
        cl = value('SELECT agent_bounded.claim();', 'bounded_executor')
        task = cl['task']
        sql('GRANT USAGE ON SCHEMA agent_supervised TO bounded_api; GRANT EXECUTE ON FUNCTION agent_supervised.command(uuid,text,jsonb) TO bounded_api;')
        bypass = {'task_id': task['id'], 'request_id': str(uuid.uuid4()), 'expected_revision': task['revision'], 'input_sha256': task['input_sha256'], 'confirmed': False, 'payload': {'action': 'inspect_input', 'material_bytes': 1}}
        sql(f"SELECT agent_supervised.command('{owner}','advance',{literal(bypass)});", 'bounded_api', True, 'bounded_task_requires_current_lease')
        sql(f"UPDATE agent_bounded.runs SET approved_until=clock_timestamp()-interval '1 second' WHERE agent_id='{guarded}';")
        sql(f"SELECT agent_bounded.advance('{cl['run_id']}','{cl['lease_id']}',{task['revision']},'{{}}');", 'bounded_executor', True, 'bounded_lease_denied')
        control(owner, guarded, 'stop')
        report['checks'].extend(['concurrent_worker_exclusion', 'owner_revocation_fences_execution', 'no_start_before_scope_approval', 'exact_scope_hash_approval', 'manual_supervised_bypass_rejected', 'inflight_authorization_expiry'])

        # Record the exact original migrations, as SQLx does, in this disposable fixture only.
        sql('CREATE TABLE _sqlx_migrations(version bigint PRIMARY KEY,description text NOT NULL,installed_on timestamptz NOT NULL DEFAULT now(),success boolean NOT NULL,checksum bytea NOT NULL,execution_time bigint NOT NULL);')
        for p in sorted((ROOT / 'migrations').glob('*.sql')):
            version = int(p.name.split('_', 1)[0])
            checksum = hashlib.sha384(p.read_bytes()).hexdigest()
            sql(f"INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES({version},'disposable exact migration',true,decode('{checksum}','hex'),0);")
        sql('GRANT USAGE ON SCHEMA public,agent_factory TO bounded_api;'
            'GRANT SELECT ON application_epoch,_sqlx_migrations,agent_access TO bounded_api;'
            'GRANT EXECUTE ON FUNCTION agent_factory.create_private(uuid,uuid,jsonb,jsonb,jsonb,jsonb),agent_factory.owned(uuid),agent_factory.managed(uuid) TO bounded_api;')
        base = dict(env)
        accounts = [{'id': owner, 'role': 'owner', 'token': uuid.uuid4().hex + uuid.uuid4().hex},
                    {'id': other, 'role': 'owner', 'token': uuid.uuid4().hex + uuid.uuid4().hex},
                    {'id': str(uuid.uuid4()), 'role': 'viewer', 'token': uuid.uuid4().hex + uuid.uuid4().hex},
                    {'id': str(uuid.uuid4()), 'role': 'owner', 'token': uuid.uuid4().hex + uuid.uuid4().hex}]
        api_port = port()
        origin = 'http://127.0.0.1:' + str(api_port)
        base.update(DATABASE_URL='postgresql://bounded_api@localhost/postgres?' + urlencode({'host': str(f['socket']), 'port': 55483}),
                    SUPA_PRIVATE_STAGING='true', SUPA_BIND='127.0.0.1:' + str(api_port), SUPA_API_TOKEN=uuid.uuid4().hex + uuid.uuid4().hex,
                    SUPA_PROVIDER='offline', SUPA_SYNTHETIC_FACTORY_ENABLED='true', SUPA_SUPERVISED_TASKS_ENABLED='true',
                    SUPA_BOUNDED_AUTONOMY_ENABLED='true', SUPA_OPERATOR_CREDENTIALS=json.dumps(accounts),
                    SUPA_OPERATOR_SESSION_DEADLINES=json.dumps({a['id']: (1 if i == 3 else int(time.time()) + 1800) for i, a in enumerate(accounts)}))
        # Expected failures are independent cases, not retries; no process can reach a provider.
        invalid = [{'SUPA_PRIVATE_STAGING': 'false'}, {'SUPA_SYNTHETIC_FACTORY_ENABLED': 'false'}, {'SUPA_SUPERVISED_TASKS_ENABLED': 'false'},
                   {'SUPA_ALLOW_OPENAI': 'true'}, {'SUPA_SUPERVISED_REAL_MODEL': 'true'}, {'SUPA_SUPERVISED_REAL_TASK_ID': str(uuid.uuid4())}]
        for i, patch in enumerate(invalid):
            r = subprocess.run([str(api)], env={**base, **patch, 'DATABASE_URL': 'not-a-database'}, capture_output=True, text=True, timeout=5)
            assert r.returncode == 1 and 'startup_or_server_failed' in r.stdout + r.stderr, 'invalid_admission_accepted'
        disabled = start(api, {**base, 'SUPA_BOUNDED_AUTONOMY_ENABLED': 'false'}, 'api-default-off')
        ready(disabled, origin, '/ready')
        assert http(origin, f'/factory/agents/{e}/autonomy', accounts[0]['token'])[0] == 403
        stop(disabled)
        active_api = start(api, base, 'api-enabled')
        ready(active_api, origin, '/ready')
        for token, expected in [(None, 401), (accounts[1]['token'], 403), (accounts[2]['token'], 403), (accounts[3]['token'], 401)]:
            assert http(origin, f'/factory/agents/{e}/autonomy', token)[0] == expected, 'http_ownership_or_expiry_boundary'
        report['checks'].extend(['six_invalid_process_configurations_rejected', 'api_default_off', 'real_api_owner_tenant_viewer_session_isolation'])

        worker_env = {**env, 'DATABASE_URL': 'postgresql://bounded_executor@localhost/postgres?' + urlencode({'host': str(f['socket']), 'port': 55483}),
                      'SUPA_PROVIDER': 'offline', 'SUPA_BOUNDED_BIND': '127.0.0.1:' + str(port())}
        worker_origin = 'http://' + worker_env['SUPA_BOUNDED_BIND']
        paused = start(M['WORKER'], worker_env, 'worker-default-paused')
        h = ready(paused, worker_origin, '/ready')
        assert h['paused'] and h['paid_requests'] == 0
        stop(paused)
        coordinator = start(M['WORKER'], {**worker_env, 'SUPA_BOUNDED_WORKER_ENABLED': 'true'}, 'worker-active')
        h = ready(coordinator, worker_origin, '/ready')
        assert not h['paused'] and h['ready']
        browser_env = {**env, 'SUPA_TEST_API_ORIGIN': origin, 'SUPA_TEST_ACCOUNTS': json.dumps([{'username': name, 'api_token': a['token']} for name, a in zip(['creator-one', 'creator-two'], accounts[:2])])}
        with (OUT / 'browser.log').open('wb') as log:
            r = subprocess.run(['/home/pikoo/.nvm/versions/node/v24.22.1/bin/node', ROOT / 'scripts/test-bounded-browser.mjs'], env=browser_env, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=240)
        assert r.returncode == 0, 'browser_journey_failed_no_retry'
        browser = json.loads((OUT / 'browser-result.json').read_text())
        assert browser['passed'] and len(browser['contexts']) == 2
        report['resources'] = {'api': resources(active_api), 'worker': resources(coordinator), 'local_uid': os.getuid()}
        assert report['resources']['worker']['peak_rss_kib'] < 256 * 1024, 'worker_memory_ceiling'
        stop(coordinator)
        stop(active_api)
        recovered_api = start(api, base, 'api-recovered')
        ready(recovered_api, origin, '/ready')
        for i, c in enumerate(browser['contexts']):
            status, data = http(origin, '/factory/agents/' + c['agent_id'] + '/autonomy', accounts[i]['token'])
            assert status == 200 and data['run']['state'] == 'stopped' and data['run']['cycles_started'] == 2
            assert data['run']['scope_sha256'] == c['scope_sha256']
            assert data['run']['cycles'][0]['approved_memory']['id'] == c['approved_memory_id']
        stop(recovered_api)
        recovered_worker = start(M['WORKER'], worker_env, 'worker-recovered-paused')
        assert ready(recovered_worker, worker_origin, '/ready')['paused']
        stop(recovered_worker)
        assert sql("SELECT count(*) FROM agent_bounded.events e WHERE previous_sha256 IS DISTINCT FROM coalesce((SELECT sha256 FROM agent_bounded.events p WHERE p.run_id=e.run_id AND p.id<e.id ORDER BY id DESC LIMIT 1),repeat('0',64));") == '0'
        assert sql("SELECT count(*) FROM agent_bounded.events WHERE body->>'operation'='review' AND body->'details'->>'result_sha256' IS NOT NULL;") != '0'
        assert sql("SELECT count(*) FROM agents WHERE data->>'lifecycle'<>'paused' OR data->'memory'<>'[]'::jsonb;") == '0'
        assert sql('SELECT count(*) FROM simulated_ledger;') == '0'
        report['checks'].extend(['real_default_paused_worker_health', 'graceful_worker_shutdown', 'desktop_mobile_browser', 'two_creators_real_factory', 'approved_memory_retrieval', 'api_restart_persistence', 'worker_paused_recovery_no_replay', 'review_hash_in_audit_chain', 'all_frozen_agents_and_accounting_unchanged'])
    finally:
        for child, log in reversed(children):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=5)
            log.close()
        report['owned_children_closed'] = all(c.poll() is not None for c, _ in children)


if __name__ == '__main__':
    M['main'].__globals__['OUT'] = OUT
    M['main'](extend)
