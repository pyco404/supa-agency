#!/usr/bin/env python3
"""New private socket-only disposable PG18 cluster. Never reads dotenv or live URLs."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid
from urllib.parse import urlencode

ROOT = Path(__file__).resolve().parents[1]
PG = ROOT / 'artifacts/ecosystem-pg18-binaries/root/usr/lib/postgresql/18/bin'

def main():
    os.umask(0o077)
    name = 'supa-autonomy-test-' + uuid.uuid4().hex
    work = Path('/tmp') / name
    ram = Path('/dev/shm') / name
    work.mkdir(mode=0o700)
    ram.mkdir(mode=0o700)
    socket, data = work / 'socket', ram / 'data'
    socket.mkdir(mode=0o700)
    env = {k: os.environ[k] for k in ['PATH','HOME']}
    env.update(LANG='C.UTF-8',LC_ALL='C.UTF-8',TZ='UTC',PGHOST=str(socket),PGPORT='55483',
               PGUSER='supa_autonomy_test',PGDATABASE='postgres',PGPASSFILE='/dev/null',PGSERVICEFILE='/dev/null',
               CARGO_TARGET_DIR=str(ROOT/'target/work-review-validation'),CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='2',
               CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0')
    # No application config or secret is inherited. Logs contain synthetic test data only.
    reports = ROOT/'artifacts/social-autonomy'
    reports.mkdir(mode=0o700,exist_ok=True)
    before = {str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (ROOT/'migrations').glob('*.sql')}
    registered = json.loads(subprocess.check_output(['pg_lsclusters','--json']))
    started = False
    def run(args,label,timeout=180):
        with (reports/(label+'.log')).open('wb') as log:
            subprocess.run([str(x) for x in args],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=timeout)
    def pg(cmd,*args):run([PG/cmd,*args],cmd,90)
    try:
        version = subprocess.check_output([str(PG/'postgres'),'--version'],env=env,text=True).strip()
        assert 'PostgreSQL) 18.' in version
        cargo=['cargo','test','--locked','--offline','-p','supa-runtime','--test','social_autonomy']
        run(cargo+['--no-run'],'build',300)
        first=['cargo','test','--locked','--offline','-p','supa-runtime','--test','x_first_post']
        run(first+['--no-run'],'first-post-build',300)
        pg('initdb','-D',data,'-U','supa_autonomy_test','--auth-local=trust','--auth-host=reject','--wal-segsize=1','--encoding=UTF8','--locale=C.UTF-8','--no-instructions')
        with (data/'postgresql.conf').open('a') as f:
            f.write(f"\nlisten_addresses=''\nunix_socket_directories='{socket}'\nport=55483\nmax_wal_size='32MB'\nmin_wal_size='2MB'\n")
        pg('pg_ctl','-D',data,'-l',reports/'postgres.log','-w','start');started=True
        pg('createdb','supa_autonomy_test')
        env['SUPA_AUTONOMY_TEST_URL']='postgresql://supa_autonomy_test@localhost/supa_autonomy_test?'+urlencode({'host':str(socket),'port':55483})
        run(cargo+['--','--exact','journey','--ignored','--nocapture'],'journey')
        run(cargo+['--','--exact','boundaries','--ignored','--nocapture'],'boundaries')
        run(cargo+['--','--exact','live_boundaries','--ignored','--nocapture'],'live-boundaries')
        run(first+['--','--exact','journey','--ignored','--nocapture'],'first-post-journey')
        pg('pg_ctl','-D',data,'-m','fast','-w','stop');started=False
        pg('pg_ctl','-D',data,'-l',reports/'postgres.log','-w','start');started=True
        run(cargo+['--','--exact','restart','--ignored','--nocapture'],'restart')
        run(first+['--','--exact','restart','--ignored','--nocapture'],'first-post-restart')
        assert before=={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (ROOT/'migrations').glob('*.sql')}
        print(json.dumps({'passed':True,'postgres':version,'mock_only':True,'live_policy_tested':True,'restart_recovery':True,'real_X_requests':0,'first_post_command_and_restart':True}))
    finally:
        if started or (data/'postmaster.pid').exists():pg('pg_ctl','-D',data,'-m','fast','-w','stop')
        unchanged = registered==json.loads(subprocess.check_output(['pg_lsclusters','--json']))
        assert not (data/'postmaster.pid').exists()
        shutil.rmtree(work)
        shutil.rmtree(ram)
        assert unchanged

if __name__=='__main__': main()
