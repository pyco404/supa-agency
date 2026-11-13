#!/usr/bin/env python3
"""One fault-injected DB timeout; synthetic, disposable, zero provider calls."""
import json
from pathlib import Path
import runpy
import subprocess
import time
from urllib.parse import urlencode

ROOT = Path(__file__).resolve().parents[1]
M = runpy.run_path(str(ROOT / 'scripts/validate-bounded-autonomy.py'))
OUT = ROOT / 'artifacts/phase5-bounded-autonomy/final-local/deadlines'


def exercise(f):
    # Check compatibility with the already-hosted reasoning extension, without inference.
    f['sql']((ROOT / 'deployment/agent-reasoning.sql').read_text())
    agent, goal = f['fixture'](f['owner'])
    f['activate'](f['owner'], agent, goal, 1)
    assert f['once']()['cycle_completed']
    assert f['command'](f['owner'], agent, 'get')['run']['state'] == 'exhausted'
    assert f['sql']('SELECT count(*) FROM agent_supervised.inferences;') == '0'
    before = f['sql']('SELECT count(*) FROM agent_bounded.cycles;')
    audit_before = f['sql']("SELECT encode(sha256(convert_to(jsonb_agg(to_jsonb(e) ORDER BY id)::text,'UTF8')),'hex') FROM agent_bounded.events e;")
    # Existing role/schema verification stays real; only the claim implementation is faulted.
    f['sql']("CREATE OR REPLACE FUNCTION agent_bounded.claim() RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$ BEGIN PERFORM pg_sleep(10); RETURN NULL; END; $$;")
    env = {**f['env'], 'DATABASE_URL': 'postgresql://bounded_executor@localhost/postgres?' + urlencode({'host': str(f['socket']), 'port': 55483}), 'SUPA_BOUNDED_WORKER_ENABLED': 'true', 'SUPA_PROVIDER': 'offline'}
    started = time.monotonic()
    r = subprocess.run([str(M['WORKER']), '--once'], cwd=ROOT, env=env, capture_output=True, text=True, timeout=6)
    elapsed = time.monotonic() - started
    assert r.returncode == 1, 'timeout_must_fail_closed'
    output = r.stdout + r.stderr
    receipts = [json.loads(line) for line in output.splitlines() if line.startswith('{')]
    failures = [entry for entry in receipts if entry.get('fields', {}).get('message') == 'bounded_worker_stopped_no_retry']
    assert len(failures) == 1, 'one_sanitized_timeout_receipt_required'
    receipt = failures[0]
    assert receipt['target'] == 'supa_bounded_worker'
    assert set(receipt['fields']) == {'error_code', 'message'}, 'private_error_fields_forbidden'
    assert receipt['fields']['error_code'] == 'provider_timeout'
    assert env['DATABASE_URL'] not in output and str(f['socket']) not in output
    assert 'pg_sleep' not in output and 'Synthetic latency' not in output
    audit_after = f['sql']("SELECT encode(sha256(convert_to(jsonb_agg(to_jsonb(e) ORDER BY id)::text,'UTF8')),'hex') FROM agent_bounded.events e;")
    assert audit_before == audit_after, 'timeout_must_preserve_durable_audit'
    (OUT / 'timeout-receipt.json').write_text(json.dumps({'exit_status': r.returncode, 'receipt': receipt, 'elapsed_seconds': round(elapsed, 3), 'audit_before': audit_before, 'audit_after': audit_after, 'retries': 0, 'paid_requests': 0}, indent=2) + '\n')
    assert 1.5 < elapsed < 5, 'database_statement_deadline_not_enforced'
    assert f['sql']('SELECT count(*) FROM agent_bounded.runs;') == '1'
    assert f['sql']('SELECT count(*) FROM agent_bounded.cycles;') == before
    f['report']['checks'] = ['deployed_reasoning_schema_compatibility', 'zero_mock_or_paid_inferences', 'real_database_statement_timeout', 'worker_exit_without_retry', 'sanitized_visible_receipt', 'durable_audit_preserved', 'no_additional_claims_or_cycles']
    f['report']['elapsed_seconds'] = round(elapsed, 3)


if __name__ == '__main__':
    M['main'].__globals__['OUT'] = OUT
    M['main'](exercise, setup_only=True)
