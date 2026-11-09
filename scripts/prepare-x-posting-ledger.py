#!/usr/bin/env python3
"""Explicit isolated PG18 X ledger preparation. No network X requests or existing DB connections."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import secrets
import socket
import stat
import subprocess
import sys
import uuid
from urllib.parse import quote, urlunsplit

ROOT=Path(__file__).resolve().parents[1]
PG=ROOT/'artifacts/ecosystem-pg18-binaries/root/usr/lib/postgresql/18/bin'
BASE=Path.home()/'.local/share/supa-agency/x-posting-ledger'
OWNER='6fd863e1-745d-4a06-b68e-eea69e854a69'
AGENT=str(uuid.uuid5(uuid.UUID(OWNER),'supa-agency/x/supa_agency/ledger-reference'))
FIXED='24fe198849ade6dfe090c1a4f4845dd2bf5218351238b4a84b1e148fdcf45c2c'
SOURCE='ae17bff87b2a7e7bc427c847db33e7b51576ba3f'
HASH='e8bc2bee94696da203aad809d8a80aeb05a4677960ef34d9b0b68b82d47f2dd7'
TEXT='Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip.'
STAGE='preflight'
DIAGNOSTIC={}

def require(ok):
    if not ok:raise ValueError('local_ledger_check_failed')

def run(args,env,input=None):
    p=subprocess.run([str(a) for a in args],input=input,env=env,cwd=ROOT,capture_output=True,timeout=90)
    if p.returncode:
        category='local_command_failed'
        if b'undefined symbol: PQsendPipelineSync' in p.stderr:category='pg18_client_loaded_incompatible_libpq'
        elif b'authentication failed' in p.stderr:category='local_authentication_denied'
        elif b'permission denied' in p.stderr:category='local_permissions_denied'
        elif b'ERROR:' in p.stderr:category='local_sql_rejected'
        DIAGNOSTIC.update(operation=Path(str(args[0])).name,exit_code=p.returncode,category=category)
        raise ValueError(category)
    return p.stdout

def write_private(path,data):
    fd=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY|os.O_NOFOLLOW,0o600)
    with os.fdopen(fd,'wb') as f:f.write(data);f.flush();os.fsync(f.fileno())

def literal(value):return "'"+str(value).replace("'","''")+"'"

def finish_existing(config):
    """Finish only this task's initialized ledger; never recreate roles or credentials."""
    global STAGE
    STAGE='prepared_ledger_identity'
    require(BASE.is_dir() and not BASE.is_symlink() and not (BASE/'setup-receipt.json').exists())
    require((BASE/'data/PG_VERSION').read_text().strip()=='18')
    original=config.private_read(ROOT/'.env')
    require((BASE/'source-environment.sha256').read_bytes()==hashlib.sha256(original).hexdigest().encode())
    profile=config.parse(config.private_read(BASE/'connection.env',4096).decode())
    require(set(profile)=={'DATABASE_URL'})
    from urllib.parse import urlsplit
    u=urlsplit(profile['DATABASE_URL'])
    require(u.scheme=='postgresql' and u.hostname=='127.0.0.1' and u.path=='/supa_x_first_post' and u.username=='supa_x_post_runtime')
    raw=config.private_read(ROOT/'artifacts/x-oauth1-private/identity-verification.json')
    evidence=json.loads(raw);account=evidence['identity']['account_id']
    require(evidence['state']=='verified' and evidence['identity']['username']=='supa_agency' and account.isdigit() and not account.startswith('0'))
    env={k:os.environ[k] for k in ('PATH','HOME')}
    env.update(LANG='C.UTF-8',LC_ALL='C.UTF-8',PGHOST=str(BASE/'socket'),PGPORT=str(u.port),PGUSER='supa_x_post_admin',PGDATABASE='supa_x_first_post',PGPASSFILE='/dev/null',PGSERVICEFILE='/dev/null',LD_LIBRARY_PATH=str(ROOT/'artifacts/ecosystem-pg18-binaries/root/usr/lib/x86_64-linux-gnu'))
    def sql(query,database='supa_x_first_post'):
        return run([PG/'psql','-X','-qAt','-v','ON_ERROR_STOP=1'],dict(env,PGDATABASE=database),query.encode()).decode().strip()
    require(sql("SELECT current_setting('data_directory');")==str(BASE/'data'))
    require(hashlib.sha256((ROOT/'deployment/social-first-post.sql').read_bytes()).hexdigest()==FIXED)
    STAGE='prepared_ledger_hba_scope'
    path=BASE/'data/pg_hba.conf';hba=path.read_text()
    hba=hba.replace('host all supa_x_post_runtime','host supa_x_first_post supa_x_post_runtime').replace('supa_x_first_post,supa_x_post_rehearsal','supa_x_first_post')
    path.write_text(hba)
    run([PG/'pg_ctl','-D',BASE/'data','reload'],env)
    account_sql=literal(account);owner=literal(OWNER)
    query="SELECT json_build_object('binding_matches',owner_id="+owner+"::uuid AND account->>'id'="+account_sql+" AND account->>'mode'='live','owner_acl',(SELECT count(*) FROM public.agent_access WHERE agent_id="+literal(AGENT)+" AND principal_id="+owner+" AND role='owner'),'drafts',(SELECT count(*) FROM social_review.drafts),'attempts',(SELECT count(*) FROM social_review.attempts),'autonomous_grants',(SELECT count(*) FROM social_autonomy.grants),'autonomous_attempts',(SELECT count(*) FROM social_autonomy.attempts),'epoch_records',(SELECT count(*) FROM public.application_epoch),'activity_records',(SELECT count(*) FROM public.activity),'only_required_public_tables',NOT EXISTS(SELECT 1 FROM pg_tables WHERE schemaname='public' AND tablename NOT IN ('agents','agent_access','cycles','activity','application_epoch')),'rehearsal_removed',NOT EXISTS(SELECT 1 FROM pg_database WHERE datname='supa_x_post_rehearsal')) FROM social_review.policies WHERE owner_id="+owner+";"
    before=json.loads(sql(query));require(before==dict(binding_matches=True,owner_acl=1,drafts=0,attempts=0,autonomous_grants=0,autonomous_attempts=0,epoch_records=0,activity_records=0,only_required_public_tables=True,rehearsal_removed=True))
    body=(ROOT/'deployment/social-first-post.sql').read_text().split('AS $$',1)[1].split('$$;',1)[0]
    expected=hashlib.sha256(body.encode()).hexdigest()
    require(sql("SELECT encode(sha256(convert_to(prosrc,'UTF8')),'hex') FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='social_review' AND p.proname='first_post';")==expected)
    STAGE='prepared_ledger_restart_test'
    run([PG/'pg_ctl','-D',BASE/'data','-m','fast','-w','stop'],env)
    run([PG/'pg_ctl','-D',BASE/'data','-l',BASE/'postgres.log','-w','start'],env)
    require(json.loads(sql(query))==before)
    STAGE='prepared_ledger_readonly_runtime_check'
    e={k:os.environ[k] for k in ('PATH','HOME','USER','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR') if k in os.environ}
    ready=json.loads(run(['python3','scripts/local-config.py','x-first-post-ledger-check','--posting-ledger',BASE/'connection.env'],e))
    require(ready['database_target_verified'] and ready['restricted_role_verified'] and ready['X_api_requests']==0)
    role_env=dict(env,PGHOST='127.0.0.1',PGUSER='supa_x_post_runtime',PGPASSFILE=str(BASE/'runtime.pgpass'))
    role=run([PG/'psql','-X','-qAt','-w'],role_env,b"SELECT current_user;").decode().strip();require(role=='supa_x_post_runtime')
    denied=subprocess.run([str(PG/'psql'),'-X','-qAt','-w','-d','postgres','-c','SELECT 1'],env=role_env,capture_output=True,timeout=10)
    require(denied.returncode!=0)
    require(config.private_read(ROOT/'.env')==original)
    manifest={'source_review':SOURCE,'postgres_version':run([PG/'postgres','--version'],env).decode().strip(),'data_directory':str(BASE/'data'),'socket_directory':str(BASE/'socket'),'loopback_port':u.port,'database':'supa_x_first_post','runtime_role':'supa_x_post_runtime','owner_id':OWNER,'ledger_reference_agent_id':AGENT,'scope':'local_x_only_not_platform_auth','account_id':account,'username':'supa_agency','identity_receipt_sha256':hashlib.sha256(raw).hexdigest(),'schema_sha256':{name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in ['deployment/x-posting-foundation.sql','deployment/ecosystem-social.sql','deployment/social-review.sql','deployment/social-autonomy.sql','deployment/social-first-post.sql']},'readiness':before,'tests':{'minimal_schema_expiry_account_text_and_confirmation':True,'duplicate_and_one_use_claim':True,'idempotent_settlement':True,'cancellation':True,'rehearsal_restart_quarantine':True,'actual_ledger_restart_persistence':True,'runtime_scram_authentication':True,'runtime_sqlx_tcp_preflight':True,'other_database_access_denied':True,'direct_table_and_ddl_denied':True},'root_dotenv_and_X_credentials_unchanged':True,'X_api_requests':0,'posting_authorizations_created':0,'autonomous_publishing_enabled':False,'console_checks':'pending_owner_confirmation','client_library_correction':'process_scoped_bundled_libpq18','tcp_address_correction':'host(inet_server_addr()) normalizes loopback CIDR'}
    write_private(BASE/'setup-receipt.json',(json.dumps(manifest,indent=2)+'\n').encode())
    print(json.dumps({'ledger_ready':True,'database':'supa_x_first_post','loopback_port':u.port,'private_setup_receipt':str(BASE/'setup-receipt.json'),'private_connection_profile':str(BASE/'connection.env'),'runtime_authentication':'SCRAM and SQLx verified','owner_binding':'existing X-only UUID verified; no platform privileges','root_dotenv_unchanged':True,'rehearsal_cleaned_up':True,'X_api_requests':0,'autonomous_publishing_enabled':False}))


class SafeParser(argparse.ArgumentParser):
    def error(self, message):
        raise ValueError('private_setup_arguments_rejected')

def main():
    global STAGE
    p=SafeParser(description=__doc__)
    flags=p.add_mutually_exclusive_group(required=True)
    flags.add_argument('--create-local-ledger',action='store_true')
    flags.add_argument('--resume-empty-local-ledger',action='store_true')
    flags.add_argument('--finish-prepared-local-ledger',action='store_true')
    args=p.parse_args();os.umask(0o077)
    spec=importlib.util.spec_from_file_location('x_ledger_local_config',ROOT/'scripts/local-config.py')
    config=importlib.util.module_from_spec(spec);spec.loader.exec_module(config)
    if args.finish_prepared_local_ledger:
        finish_existing(config)
        return
    original_env=config.private_read(ROOT/'.env')
    identity_bytes=config.private_read(ROOT/'artifacts/x-oauth1-private/identity-verification.json')
    evidence=json.loads(identity_bytes);identity=evidence['identity'];account=identity['account_id']
    require(evidence['state']=='verified' and evidence['request_count']==1 and identity['username']=='supa_agency' and identity['auth_mode']=='oauth1')
    require(account.isascii() and account.isdigit() and not account.startswith('0') and 0<int(account)<2**64)
    require(hashlib.sha256((ROOT/'deployment/social-first-post.sql').read_bytes()).hexdigest()==FIXED)
    required=['deployment/ecosystem-social.sql','deployment/social-review.sql','deployment/social-autonomy.sql','deployment/social-first-post.sql']
    for name in required:
        reviewed=subprocess.check_output([str(ROOT/'scripts/git-local'),'show',SOURCE+':'+name],cwd=ROOT)
        require(reviewed==(ROOT/name).read_bytes())
    require(not BASE.is_symlink() and os.getuid()!=0)
    if args.resume_empty_local_ledger:
        require(BASE.is_dir() and (BASE/'data/PG_VERSION').read_text().strip()=='18' and not (BASE/'data/postmaster.pid').exists())
        require(not (BASE/'connection.env').exists() and not (BASE/'runtime.pgpass').exists())
        require((BASE/'source-environment.sha256').read_bytes()==hashlib.sha256(original_env).hexdigest().encode())
    else:require(not BASE.exists())
    require(not any(p.is_symlink() for p in BASE.parents))
    registered=json.loads(run(['pg_lsclusters','--json'],os.environ.copy()))
    if args.resume_empty_local_ledger:
        lines=(BASE/'data/postgresql.conf').read_text().splitlines()
        port=int([s.split('=',1)[1] for s in lines if s.startswith('port=')][-1])
    else:
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    env={k:os.environ[k] for k in ('PATH','HOME') if k in os.environ}
    env.update(LANG='C.UTF-8',LC_ALL='C.UTF-8',TZ='UTC',PGHOST=str(BASE/'socket'),PGPORT=str(port),PGUSER='supa_x_post_admin',PGPASSFILE='/dev/null',PGSERVICEFILE='/dev/null')
    env['LD_LIBRARY_PATH']=str(ROOT/'artifacts/ecosystem-pg18-binaries/root/usr/lib/x86_64-linux-gnu')
    require(b'18.' in run([PG/'psql','--version'],env))
    version=run([PG/'postgres','--version'],env).decode().strip();require('PostgreSQL) 18.' in version)
    if not args.resume_empty_local_ledger:
        BASE.mkdir(parents=True,mode=0o700);(BASE/'socket').mkdir(mode=0o700)
        write_private(BASE/'source-environment.sha256',hashlib.sha256(original_env).hexdigest().encode())
    started=False
    def sql(database,statement,runtime=False):
        e=dict(env,PGDATABASE=database)
        if runtime:e.update(PGHOST='127.0.0.1',PGUSER='supa_x_post_runtime',PGPASSFILE=str(BASE/'runtime.pgpass'))
        # Only the new isolated cluster is reachable through these explicit socket/port settings.
        check="DO $$ BEGIN IF current_database()<>"+literal(database)+" THEN RAISE EXCEPTION 'wrong_local_database'; END IF; END $$;\n"
        if not runtime:check+="DO $$ BEGIN IF current_setting('data_directory')<>"+literal(BASE/'data')+" THEN RAISE EXCEPTION 'wrong_local_cluster'; END IF; END $$;\n"
        return run([PG/'psql','-X','-qAt','-v','ON_ERROR_STOP=1'],e,(check+statement).encode())
    try:
        STAGE='isolated_cluster_init'
        if not args.resume_empty_local_ledger:run([PG/'initdb','-D',BASE/'data','-U','supa_x_post_admin','--auth-local=peer','--auth-host=scram-sha-256','--wal-segsize=1','--encoding=UTF8','--locale=C.UTF-8','--no-instructions'],env)
        username=pwd.getpwuid(os.getuid()).pw_name
        (BASE/'data/pg_ident.conf').write_text('x_ledger_admin '+username+' supa_x_post_admin\n')
        (BASE/'data/pg_hba.conf').write_text('local all supa_x_post_admin peer map=x_ledger_admin\nlocal all all reject\nhost supa_x_first_post,supa_x_post_rehearsal supa_x_post_runtime 127.0.0.1/32 scram-sha-256\nhost all all 127.0.0.1/32 reject\nhost all all ::1/128 reject\n')
        with (BASE/'data/postgresql.conf').open('a') as f:
            f.write("\nlisten_addresses='127.0.0.1'\nport="+str(port)+"\nunix_socket_directories='"+str(BASE/'socket')+"'\nmax_connections=10\nshared_buffers='16MB'\nmax_wal_size='32MB'\nmin_wal_size='2MB'\npassword_encryption='scram-sha-256'\nlog_statement='none'\nlog_min_error_statement='panic'\nlog_error_verbosity='terse'\n")
        run([PG/'pg_ctl','-D',BASE/'data','-l',BASE/'postgres.log','-w','start'],env);started=True
        STAGE='restricted_local_roles'
        if args.resume_empty_local_ledger:
            empty=sql('postgres',"SELECT (SELECT count(*) FROM pg_roles WHERE rolname NOT LIKE 'pg_%' AND rolname<>'supa_x_post_admin')=0 AND (SELECT count(*) FROM pg_database WHERE datname NOT IN ('postgres','template0','template1'))=0;").decode().strip()
            require(empty=='t')
        password=secrets.token_hex(32)
        sql('postgres',"CREATE ROLE supa_x_post_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;\nCREATE ROLE supa_x_post_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 2 PASSWORD "+literal(password)+";\nREVOKE CONNECT ON DATABASE postgres FROM PUBLIC;\nCREATE DATABASE supa_x_post_rehearsal OWNER supa_x_post_owner;\nCREATE DATABASE supa_x_first_post OWNER supa_x_post_owner;")
        write_private(BASE/'runtime.pgpass',('\n'.join('127.0.0.1:'+str(port)+':'+db+':supa_x_post_runtime:'+password for db in ['supa_x_first_post','supa_x_post_rehearsal'])+'\n').encode())
        authority='supa_x_post_runtime:'+quote(password,safe='')+'@127.0.0.1:'+str(port)
        url=urlunsplit(('postgresql',authority,'/supa_x_first_post','',''))
        require(config.loopback_url(url,['postgresql'],True))
        write_private(BASE/'connection.env',('DATABASE_URL='+url+'\n').encode())
        password=None;url=None
        STAGE='minimal_x_schema'
        foundation=(ROOT/'deployment/x-posting-foundation.sql').read_text()
        def install(db,account_id,mode):
            sql(db,"REVOKE ALL ON DATABASE "+db+" FROM PUBLIC; GRANT CONNECT ON DATABASE "+db+" TO supa_x_post_runtime;\nSET ROLE supa_x_post_owner;\n"+foundation)
            for name in required:sql(db,'SET ROLE supa_x_post_owner;\n'+(ROOT/name).read_text())
            payload=json.dumps({'name':'Supa Agent','kind':'x_only_ledger_reference','reference_only':True,'execution_enabled':False,'spending_authority':0})
            a=json.dumps({'id':account_id,'username':'supa_agency','mode':mode})
            sql(db,"SET ROLE supa_x_post_owner; BEGIN;\nINSERT INTO public.agents(id,data) VALUES("+literal(AGENT)+","+literal(payload)+"::jsonb);\nINSERT INTO public.agent_access(agent_id,principal_id,role,granted_by) VALUES("+literal(AGENT)+","+literal(OWNER)+",'owner',"+literal(OWNER)+");\nINSERT INTO social_review.policies(agent_id,owner_id,account,enabled) VALUES("+literal(AGENT)+","+literal(OWNER)+","+literal(a)+"::jsonb,true);\nINSERT INTO social_review.pauses(agent_id,paused) VALUES("+literal(AGENT)+",false);\nGRANT USAGE ON SCHEMA social_review TO supa_x_post_runtime; GRANT EXECUTE ON FUNCTION social_review.first_post(uuid,text,jsonb) TO supa_x_post_runtime; COMMIT;")
        install('supa_x_post_rehearsal','777','mock')
        STAGE='rehearsal_authorization_and_settlement'
        body=json.dumps({'authorization_id':str(uuid.uuid4()),'account':{'id':'777','username':'supa_agency','mode':'mock'},'text':TEXT,'text_sha256':HASH,'budget_microusd':25000,'confirmed':True,'read_write_confirmed':True,'credits_confirmed':True,'identity_receipt_sha256':'a'*64})
        owner=literal(OWNER)
        tests="""BEGIN; SET ROLE supa_x_post_runtime;
DO $test$ DECLARE b jsonb:=%s::jsonb; changed jsonb; k text; r jsonb; BEGIN
 b=b||jsonb_build_object('expires_at',clock_timestamp()+interval '5 minutes');
 FOREACH k IN ARRAY ARRAY['confirmed','read_write_confirmed','credits_confirmed'] LOOP
  BEGIN PERFORM social_review.first_post(%s,'reserve',b||jsonb_build_object(k,false)); RAISE EXCEPTION 'expected_rejection'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 FOREACH k IN ARRAY ARRAY['expired','account','text'] LOOP
  changed=CASE k WHEN 'expired' THEN b||jsonb_build_object('expires_at',clock_timestamp()-interval '1 second') WHEN 'account' THEN b||jsonb_build_object('account',jsonb_build_object('id','778','username','supa_agency','mode','mock')) ELSE b||jsonb_build_object('text','changed') END;
  BEGIN PERFORM social_review.first_post(%s,'reserve',changed); RAISE EXCEPTION 'expected_rejection'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 r=social_review.first_post(%s,'reserve',b); IF r->>'fresh'<>'true' THEN RAISE EXCEPTION 'reserve_failed'; END IF;
 r=social_review.first_post(%s,'reserve',b); IF r->>'fresh'<>'false' THEN RAISE EXCEPTION 'duplicate_not_fenced'; END IF;
 PERFORM social_review.first_post(%s,'peek',b); PERFORM social_review.first_post(%s,'peek',b);
 PERFORM social_review.first_post(%s,'claim',b);
 BEGIN PERFORM social_review.first_post(%s,'claim',b); RAISE EXCEPTION 'claim_replayed'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 r=jsonb_build_object('mode','mock','state','posted','category','mock_post_receipt','post_id','456','http_status',201,'rate_limit_reset',NULL);
 PERFORM social_review.first_post(%s,'settle',b||jsonb_build_object('receipt',r));
 PERFORM social_review.first_post(%s,'settle',b||jsonb_build_object('receipt',r));
END $test$; ROLLBACK;
""" % tuple([literal(body)]+[owner]*10)
        sql('supa_x_post_rehearsal',tests)
        # Cancellation remains durable inside the transaction, then fixture changes are rolled back.
        cancel="BEGIN; SET ROLE supa_x_post_runtime; DO $t$ DECLARE b jsonb:="+literal(body)+"::jsonb; BEGIN b=b||jsonb_build_object('expires_at',clock_timestamp()+interval '5 minutes'); PERFORM social_review.first_post("+owner+",'reserve',b); PERFORM social_review.first_post("+owner+",'cancel',b); BEGIN PERFORM social_review.first_post("+owner+",'claim',b); RAISE EXCEPTION 'cancel_bypassed'; EXCEPTION WHEN insufficient_privilege THEN NULL; END; END $t$; ROLLBACK;"
        sql('supa_x_post_rehearsal',cancel)
        pending=json.loads(body);pending['authorization_id']=str(uuid.uuid4())
        persist="SET ROLE supa_x_post_runtime; SELECT social_review.first_post("+owner+",'reserve',"+literal(json.dumps(pending))+"::jsonb||jsonb_build_object('expires_at',clock_timestamp()+interval '5 minutes')); SELECT social_review.first_post("+owner+",'claim',"+literal(json.dumps(pending))+"::jsonb);"
        sql('supa_x_post_rehearsal',persist)
        STAGE='isolated_restart_recovery'
        run([PG/'pg_ctl','-D',BASE/'data','-m','fast','-w','stop'],env);started=False
        run([PG/'pg_ctl','-D',BASE/'data','-l',BASE/'postgres.log','-w','start'],env);started=True
        result=sql('supa_x_post_rehearsal',"SET ROLE supa_x_post_runtime; SELECT social_review.first_post("+owner+",'recover',"+literal(json.dumps(pending))+"::jsonb);").decode().strip()
        require(json.loads(result)['state']=='uncertain')
        check=sql('supa_x_post_rehearsal',"SELECT state FROM social_review.attempts WHERE draft_id="+literal(pending['authorization_id'])+";").decode().strip();require(check=='uncertain')
        STAGE='live_scope_ledger_install_no_post_authorization'
        install('supa_x_first_post',account,'live')
        proof=sql('supa_x_first_post',"SELECT json_build_object('binding_matches',owner_id="+owner+"::uuid AND account->>'id'="+literal(account)+" AND account->>'mode'='live','owner_acl',(SELECT count(*) FROM public.agent_access WHERE agent_id="+literal(AGENT)+" AND principal_id="+owner+" AND role='owner'),'pending_attempts',(SELECT count(*) FROM social_review.attempts),'autonomous_grants',(SELECT count(*) FROM social_autonomy.grants),'epoch_records',(SELECT count(*) FROM public.application_epoch)) FROM social_review.policies WHERE owner_id="+owner+";").decode().strip()
        proof=json.loads(proof);require(proof==dict(binding_matches=True,owner_acl=1,pending_attempts=0,autonomous_grants=0,epoch_records=0))
        role=sql('supa_x_first_post',"SELECT current_database()||':'||current_user;",runtime=True).decode().strip();require(role=='supa_x_first_post:supa_x_post_runtime')
        for statement in ['SELECT * FROM public.agents;','SELECT * FROM social_review.drafts;','UPDATE public.agent_access SET role=\'owner\';','CREATE TABLE public.not_allowed(id integer);']:
            try:sql('supa_x_first_post',statement,runtime=True)
            except ValueError:pass
            else:raise ValueError('runtime_privilege_excess')
        STAGE='rehearsal_cleanup_and_readonly_configuration_check'
        sql('postgres','DROP DATABASE supa_x_post_rehearsal;')
        hba=(BASE/'data/pg_hba.conf').read_text().replace('supa_x_first_post,supa_x_post_rehearsal','supa_x_first_post')
        (BASE/'data/pg_hba.conf').write_text(hba)
        run([PG/'pg_ctl','-D',BASE/'data','reload'],env)
        # Remove the discarded rehearsal credential entry without replacing any preexisting file.
        passpath=BASE/'runtime.pgpass';lines=passpath.read_text().splitlines();passpath.write_text('\n'.join(line for line in lines if ':supa_x_first_post:' in line)+'\n');passpath.chmod(0o600)
        require(config.private_read(ROOT/'.env')==original_env)
        result=run(['python3','scripts/local-config.py','x-first-post-ledger-check','--posting-ledger',BASE/'connection.env'],{k:os.environ[k] for k in ('PATH','HOME','USER','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR') if k in os.environ})
        ready=json.loads(result);require(ready['database_target_verified'] and ready['restricted_role_verified'] and ready['X_api_requests']==0)
        require(json.loads(run(['pg_lsclusters','--json'],os.environ.copy()))==registered)
        receipt={'version':1,'source_review':SOURCE,'postgres_version':version,'data_directory':str(BASE/'data'),'socket_directory':str(BASE/'socket'),'loopback_port':port,'database':'supa_x_first_post','runtime_role':'supa_x_post_runtime','owner_id':OWNER,'ledger_reference_agent_id':AGENT,'scope':'local_x_only_not_platform_auth','account_id':account,'username':'supa_agency','identity_receipt_sha256':hashlib.sha256(identity_bytes).hexdigest(),'schema_sha256':{name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in ['deployment/x-posting-foundation.sql']+required},'binding':proof,'tests':{'expiry':True,'exact_account_and_text':True,'confirmation':True,'duplicate_prevention':True,'read_only_peek_and_one_use_claim':True,'idempotent_settlement':True,'cancellation':True,'physical_restart_recovery':True,'runtime_scram_login':True,'direct_table_and_ddl_denied':True,'rehearsal_database_removed':True},'root_dotenv_unchanged':True,'existing_registered_clusters_unchanged':True,'X_api_requests':0,'autonomous_publishing_enabled':False,'posting_authorizations_created':0,'permissions_and_credits':'owner_console_confirmation_required'}
        write_private(BASE/'setup-receipt.json',(json.dumps(receipt,indent=2)+'\n').encode())
        print(json.dumps({'ledger_ready':True,'database':'supa_x_first_post','loopback_port':port,'private_setup_receipt':str(BASE/'setup-receipt.json'),'private_connection_profile':str(BASE/'connection.env'),'runtime_authentication':'SCRAM verified','owner_binding':'existing X-only UUID verified; no platform privileges','root_dotenv_unchanged':True,'existing_clusters_unchanged':True,'rehearsal_cleaned_up':True,'X_api_requests':0,'autonomous_publishing_enabled':False}))
    except Exception:
        if started:
            try:run([PG/'pg_ctl','-D',BASE/'data','-m','fast','-w','stop'],env)
            except Exception:pass
        raise

if __name__=='__main__':
    try:main()
    except Exception:
        print(json.dumps({'stopped':True,'stage':STAGE,'diagnostics':'bounded sanitized local setup failure; no automatic retry','X_api_requests':0,'sanitized_diagnostic':DIAGNOSTIC}),file=sys.stderr);sys.exit(1)
