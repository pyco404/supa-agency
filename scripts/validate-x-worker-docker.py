#!/usr/bin/env python3
"""Disposable Docker rehearsal; existing mock image only, no pulls/builds/live secrets."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report-dir', type=Path, default=Path('artifacts/x-worker-container'))
    options = parser.parse_args()
    image = 'supa-x-worker:mock-local'
    docker = os.environ.get('SUPA_X_DOCKER_BINARY', 'docker')
    name = 'supa-x-rehearsal-' + uuid.uuid4().hex
    volume = name + '-data'
    report = {'mock_only': True, 'X_requests': 0, 'checks': []}
    with tempfile.TemporaryDirectory(prefix='supa-x-docker-client-') as home:
        env = {'PATH': '/usr/bin:/bin', 'HOME': home}
        if 'DOCKER_HOST' in os.environ:
            env['DOCKER_HOST'] = os.environ['DOCKER_HOST']
        def run(*args, allowed=(0,), timeout=40, category=None):
            r = subprocess.run([docker, *args], env=env, capture_output=True, text=True, timeout=timeout)
            if r.returncode not in allowed:
                raise RuntimeError('docker_operation_failed_' + args[0])
            if category is not None:
                assert json.loads(r.stderr)['category'] == category
            return r.stdout.strip()
        def inside(*args, **kwargs):
            return run('exec', name, *args, **kwargs)
        def sql(query):
            return inside('psql', '-XAt', '-v', 'ON_ERROR_STOP=1', '-h', '/etc/supa-agent/x-worker/socket',
                          '-p', '55483', '-U', 'supa_x_mock_admin', '-d', 'supa_x_first_post', '-c', query)
        def control(operation, **kwargs):
            return inside('python3', '/opt/supa-x/entrypoint.py', 'control', operation, **kwargs)
        def wait(check, timeout=70):
            end = time.monotonic() + timeout
            while time.monotonic() < end:
                try:
                    if check():
                        return
                except (RuntimeError, json.JSONDecodeError):
                    pass
                time.sleep(0.5)
            raise RuntimeError('bounded_rehearsal_deadline_exhausted')
        def health():
            return json.loads(inside('python3', '-c', "import urllib.request;print(urllib.request.urlopen('http://127.0.0.1:8080/healthz',timeout=2).read().decode())"))
        def check(label):
            report['checks'].append(label)
            print(json.dumps({'passed': label}), flush=True)
        created = False
        try:
            info = json.loads(run('image', 'inspect', image))[0]
            assert info['Config']['User'] == '10001:10001'
            assert info['Config']['StopSignal'] == 'SIGTERM'
            report['image_id'] = info['Id']
            assert run('run', '--rm', '--network', 'none', '--entrypoint', 'cat', image, '/opt/supa-x/bundle-mode') == 'mock'
            run('run', '--rm', '--network', 'none', '--entrypoint', 'python3', image, '-c',
                "from pathlib import Path;assert not Path('/opt/supa-x/bin/supa-x-autonomy').exists();assert not Path('/opt/supa-x/bin/supa-x-first-post').exists()")
            check('nonroot_image_without_live_dispatcher')
            # Keep the actual installed-package inventory available to scanners.
            # PostgreSQL initdb/pg_ctl require sh; Bash, build tools, the package
            # manager and privilege-switching helper are not runtime dependencies.
            runtime_probe = textwrap.dedent('''
                import json, os, shutil, ssl
                from pathlib import Path
                assert Path('/lib/apk/db/installed').is_file()
                assert 'wolfi' in Path('/etc/os-release').read_text()
                assert all(shutil.which(p) is None for p in
                           ('apk', 'bash', 'gosu', 'cargo', 'rustc', 'gcc', 'cc', 'make', 'clang', 'git'))
                assert not Path('/usr/local/bin/gosu').exists()
                ctx = ssl.create_default_context()
                assert ctx.verify_mode == ssl.CERT_REQUIRED and ctx.check_hostname
                assert len(ctx.get_ca_certs()) > 0
                print(json.dumps({'libc': os.confstr('CS_GNU_LIBC_VERSION'),
                                  'tls': ssl.OPENSSL_VERSION,
                                  'trusted_ca_count': len(ctx.get_ca_certs())}))
            ''')
            runtime = json.loads(run('run', '--rm', '--network', 'none', '--entrypoint', 'python3', image,
                                     '-c', runtime_probe))
            report['runtime'] = runtime
            check('minimal_glibc_runtime_without_build_tools_and_with_verified_tls_defaults')
            run('volume', 'create', volume)
            created = True
            # Owner-only initialization of a new disposable volume. The publisher
            # itself always runs as UID 10001, never with RAILWAY_RUN_UID=0.
            run('run', '--rm', '--network', 'none', '--user', '0', '--entrypoint', 'python3',
                '--mount', 'type=volume,src=' + volume + ',dst=/etc/supa-agent', image, '-c',
                "import os;os.makedirs('/etc/supa-agent/x-worker',mode=0o700,exist_ok=True);os.chown('/etc/supa-agent/x-worker',10001,10001);os.chmod('/etc/supa-agent/x-worker',0o700)")
            run('run', '-d', '--name', name, '--network', 'none', '--cap-drop', 'ALL',
                '--security-opt', 'no-new-privileges=true', '--read-only', '--tmpfs', '/tmp:rw,noexec,nosuid,size=32m',
                '--mount', 'type=volume,src=' + volume + ',dst=/etc/supa-agent',
                '-e', 'SUPA_X_WORKER_MODE=mock', image)
            wait(lambda: health()['state'] == 'paused')
            assert inside('id', '-u') == '10001'
            assert sql('SELECT count(*) FROM social_autonomy.attempts') == '0'
            check('actual_container_boots_paused_with_private_pg18')
            r = run('exec', name, 'python3', '/opt/supa-x/entrypoint.py', allowed=(1,), category='publisher_already_running')
            assert not r
            check('second_publisher_rejected_before_pg_start')
            control('resume')
            wait(lambda: sql("SELECT count(*) FROM social_autonomy.attempts WHERE state='posted'") == '1')
            time.sleep(2)
            assert sql('SELECT count(*) FROM social_autonomy.attempts') == '1'
            check('approved_mock_dispatch_and_duplicate_spacing_fence')
            generation = health()['lease_generation']
            run('restart', '--time', '30', name, timeout=50)
            wait(lambda: health()['state'] == 'active_mock')
            assert health()['lease_generation'] == generation + 1
            assert sql('SELECT count(*) FROM social_autonomy.attempts') == '1'
            check('unchanged_unexpired_authorization_survives_clean_restart_without_repost')
            control('pause')
            wait(lambda: health()['state'].startswith('paused'))
            sql("UPDATE social_autonomy.attempts SET created_at=clock_timestamp()-interval '5 hours'; UPDATE social_autonomy.grants SET next_at=clock_timestamp()-interval '1 second';")
            sql("""INSERT INTO activity(id,agent_id,kind,code,simulated) VALUES('00000000-0000-0000-0000-0000000000c9','00000000-0000-0000-0000-00000000000a','goal','added',true);
              INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released)
              SELECT '00000000-0000-0000-0000-00000000000a',(s->'event'->>'id')::uuid,'00000000-0000-0000-0000-000000000001',s->>'snapshot_sha256',true
              FROM jsonb_array_elements(social_review.context('00000000-0000-0000-0000-00000000000a')->'sources') s WHERE s->'event'->>'id'='00000000-0000-0000-0000-0000000000c9';""")
            # Keep supervisor paused; invoke one explicit mock fault, no racing tick.
            inside('/opt/supa-x/bin/supa-x-mock-worker', 'resume', allowed=(1,))  # No DB environment: fails closed.
            # Resume only the mock SQL grant, leaving the supervisor paused. This
            # avoids racing its observer when injecting exactly one mock fault.
            inside('python3', '-c', "import json,subprocess;from pathlib import Path;m=json.loads(Path('/etc/supa-agent/x-worker/ledger.json').read_text());subprocess.run(['/opt/supa-x/bin/supa-x-mock-worker','resume'],env={'DATABASE_URL':'postgresql://supa_x_post_runtime@127.0.0.1:55483/supa_x_first_post','SUPA_X_MOCK_EXPIRES_AT':m['expires_at']},check=True)")
            control('tick-uncertain')
            assert sql("SELECT count(*) FROM social_autonomy.attempts WHERE state='uncertain'") == '1'
            control('resume', allowed=(1,))
            before = sql("SELECT encode(sha256(convert_to(jsonb_agg(to_jsonb(a) ORDER BY id)::text,'UTF8')),'hex') FROM social_autonomy.attempts a")
            check('ambiguous_mock_outcome_quarantines_without_retry')
            generation = health()['lease_generation']
            run('restart', '--time', '30', name, timeout=50)
            wait(lambda: health()['state'] == 'paused')
            assert health()['lease_generation'] == generation + 1
            after = sql("SELECT encode(sha256(convert_to(jsonb_agg(to_jsonb(a) ORDER BY id)::text,'UTF8')),'hex') FROM social_autonomy.attempts a")
            assert before == after
            control('resume', allowed=(1,))
            assert sql("SELECT count(*) FROM (SELECT previous_sha256,coalesce(lag(sha256) over(order by id),repeat('0',64)) expected FROM social_autonomy.events) q WHERE previous_sha256<>expected") == '0'
            check('actual_restart_retains_history_hashes_and_quarantine')
            assert run('inspect', name, '--format', '{{.HostConfig.NetworkMode}}') == 'none'
            report['rss_kib'] = int(inside('python3', '-c', "from pathlib import Path;print(sum(int(l.split()[1]) for p in Path('/proc').glob('[0-9]*/status') for l in p.read_text().splitlines() if l.startswith('VmRSS:')))") or '0')
            # This rootless rehearsal measures memory but cannot attest Railway
            # cgroup delegation or enforce the future service resource ceilings.
            report['resource_limits_enforced'] = False
            run('stop', '--time', '30', name, timeout=45)
            assert run('inspect', name, '--format', '{{.State.ExitCode}}') == '0'
            check('graceful_sigterm_stops_worker_before_postgresql')
            report['passed'] = True
        finally:
            run('rm', '-f', '-v', name, allowed=(0, 1), timeout=30)
            if created:
                run('volume', 'rm', volume)
            report['temporary_resources_removed'] = True
            out = options.report_dir
            out.mkdir(mode=0o700, parents=True, exist_ok=True)
            (out / 'rehearsal.json').write_text(json.dumps(report, indent=2) + '\n')
            print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
