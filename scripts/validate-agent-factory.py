#!/usr/bin/env python3
"""Opt-in, new socket-only PG18 cluster. Never accepts existing DB URLs or paths."""
import hashlib,json,os,pathlib,subprocess,uuid,shutil,sys
from urllib.parse import urlencode
ROOT=pathlib.Path(__file__).resolve().parents[1]
PG=ROOT/'artifacts/ecosystem-pg18-binaries/root/usr/lib/postgresql/18/bin'
OUT=ROOT/'artifacts/functional-factory'
def main():
    os.umask(0o077);OUT.mkdir(exist_ok=True,parents=True)
    work=pathlib.Path('/tmp')/('supa-factory-test-'+uuid.uuid4().hex);work.mkdir(mode=0o700);socket=work/'socket';socket.mkdir(mode=0o700)
    # Keep the strict /tmp socket guard. Only this invocation's data/build cache
    # may live in private RAM when the workspace disk cannot hold a fresh cluster.
    ram=pathlib.Path('/dev/shm')/work.name if os.environ.get('SUPA_FACTORY_TEST_RAM')=='1' else None
    if ram:
        ram.mkdir(mode=0o700)
        (ram/'tmp').mkdir(mode=0o700)
    data=(ram or work)/'data'
    env={k:os.environ[k] for k in ['PATH','HOME']};env.update(LANG='C.UTF-8',LC_ALL='C.UTF-8',TZ='UTC',PGHOST=str(socket),PGPORT='55481',PGUSER='supa_factory_test',PGDATABASE='postgres',PGPASSFILE='/dev/null',PGSERVICEFILE='/dev/null',CARGO_TARGET_DIR=str(ram/'cargo' if ram else ROOT/'target/work-review-validation'),CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='2',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',SUPA_FACTORY_BROWSER='1',SUPA_FACTORY_AVATARS='1')
    if ram:env['TMPDIR']=str(ram/'tmp')
    registered=json.loads(subprocess.check_output(['pg_lsclusters','--json']))
    hashes={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((ROOT/'migrations').glob('*.sql'))}
    def run(args,log,timeout=600):
        with (OUT/log).open('ab') as output:subprocess.run([str(a) for a in args],cwd=ROOT,env=env,stdout=output,stderr=subprocess.STDOUT,check=True,timeout=timeout)
    def pg(command,*args):run([PG/command,*args],command+'.log',90)
    version=subprocess.check_output([str(PG/'postgres'),'--version'],env=env,text=True).strip();assert 'PostgreSQL) 18.' in version
    cargo=['cargo','test','--locked','--offline','-p','supa-api','--test','factory_journey']
    started=False
    try:
        if ram:shutil.copytree(ROOT/'target/work-review-validation',ram/'cargo')
        run(['/home/pikoo/.nvm/versions/node/v24.22.1/bin/node','scripts/test-avatar-images.mjs'],'avatar-codec.log')
        run(cargo+['--no-run','--message-format=json'],'build.log')
        test=cargo+['--']
        if ram:
            # Execute the exact Cargo-reported test artifact; release only our
            # copied cache before Chrome starts, avoiding RAM pressure/swapping.
            messages=[json.loads(line) for line in (OUT/'build.log').read_text().splitlines() if line.startswith('{')]
            built=next(m for m in reversed(messages) if m.get('reason')=='compiler-artifact' and m.get('target',{}).get('name')=='factory_journey' and m.get('executable'))
            binary=pathlib.Path(built['executable'])
            assert binary.parent==ram/'cargo/debug/deps' and binary.is_file() and not binary.is_symlink()
            shutil.copy2(binary,ram/'factory-journey')
            shutil.rmtree(ram/'cargo')
            test=[ram/'factory-journey']
        run(test+['refuses_unscoped_database_targets'],'target-guard.log')
        pg('initdb','-D',data,'-U','supa_factory_test','--auth-local=trust','--auth-host=reject','--wal-segsize=1','--encoding=UTF8','--locale=C.UTF-8','--no-instructions')
        with (data/'postgresql.conf').open('a') as f:f.write(f"\nlisten_addresses=''\nunix_socket_directories='{socket}'\nport=55481\nmax_wal_size='32MB'\nmin_wal_size='2MB'\n")
        pg('pg_ctl','-D',data,'-l',OUT/'postgres.log','-w','start');started=True;pg('createdb','supa_factory_test')
        env['SUPA_FACTORY_TEST_URL']='postgresql://supa_factory_test@localhost/supa_factory_test?'+urlencode({'host':str(socket),'port':55481})
        run(test+['--exact','factory_journey','--include-ignored','--nocapture'],'journey.log')
        pg('pg_ctl','-D',data,'-m','fast','-w','stop');started=False
        pg('pg_ctl','-D',data,'-l',OUT/'postgres.log','-w','start');started=True
        run(test+['--exact','factory_restart_recovery','--include-ignored','--nocapture'],'recovery.log')
        if '--supervised' in sys.argv:
            run(test+['--exact','supervised_journey','--include-ignored','--nocapture'],'supervised.log')
            pg('pg_ctl','-D',data,'-m','fast','-w','stop');started=False
            pg('pg_ctl','-D',data,'-l',OUT/'postgres.log','-w','start');started=True
            run(test+['--exact','supervised_restart_recovery','--include-ignored','--nocapture'],'supervised-recovery.log')
        assert hashes=={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((ROOT/'migrations').glob('*.sql'))}
        (OUT/'postgres-result.json').write_text(json.dumps({'passed':True,'postgres_version':version,'socket_only':True,'disposable_cluster':str(work),'real_restart_recovery':True,'restricted_role':True,'concurrent_idempotency':True,'atomic_rollback':True,'existing_migrations_unchanged':True,'railway_operations':0},indent=2)+'\n')
    finally:
        if started or (data/'postmaster.pid').exists():pg('pg_ctl','-D',data,'-m','fast','-w','stop')
        assert registered==json.loads(subprocess.check_output(['pg_lsclusters','--json']))
        assert not (data/'postmaster.pid').exists()
        shutil.rmtree(work)  # Only this invocation's stopped disposable fixture; logs retained.
        if ram:shutil.rmtree(ram)
    print(json.dumps({'passed':True,'evidence':str(OUT),'disposable_stopped':True}))
if __name__=='__main__':main()
