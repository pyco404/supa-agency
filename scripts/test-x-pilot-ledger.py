#!/usr/bin/env python3
"""Disposable socket-only PostgreSQL18. No real ledger, credentials or X calls."""
import hashlib,json,os,pathlib,subprocess,tempfile,unittest,uuid
ROOT=pathlib.Path(__file__).resolve().parents[1];PG=ROOT/'artifacts/ecosystem-pg18-binaries/root/usr/lib/postgresql/18/bin'
class Ledger(unittest.TestCase):
 @classmethod
 def setUpClass(cls):
  cls.tmp=tempfile.TemporaryDirectory(prefix='supa-pilot-test-',dir='/tmp');cls.root=pathlib.Path(cls.tmp.name);cls.data=cls.root/'data';cls.socket=cls.root/'socket';cls.socket.mkdir(mode=0o700)
  cls.env={'PATH':'/usr/bin:/bin','LANG':'C.UTF-8','HOME':str(cls.root),'PGPASSFILE':'/dev/null','PGSERVICEFILE':'/dev/null','LD_LIBRARY_PATH':str(ROOT/'artifacts/ecosystem-pg18-binaries/root/usr/lib/x86_64-linux-gnu')}
  cls.command('initdb','-D',str(cls.data),'-U','supa_pilot_admin','--auth-local=trust','--auth-host=reject','--no-instructions')
  cls.command('pg_ctl','-D',str(cls.data),'-l',str(cls.root/'postgres.log'),'-o',f'-k {cls.socket} -p 55487 -c listen_addresses=','-w','start')
  try:
   # Roles are cluster-wide; create the same restricted login once. Per-test
   # databases independently receive unchanged production grants and triggers.
   cls.sql('CREATE ROLE supa_x_post_runtime LOGIN;',database='postgres')
  except BaseException:
   cls.command('pg_ctl','-D',str(cls.data),'-m','fast','-w','stop');cls.tmp.cleanup();raise
 @classmethod
 def command(cls,name,*args):
  assert cls.root.parent==pathlib.Path('/tmp') and cls.root.name.startswith('supa-pilot-test-')
  return subprocess.run([str(PG/name),*args],env=cls.env,capture_output=True,text=True,check=True,timeout=30).stdout
 @classmethod
 def sql(cls,text,deny=False,database='supa_x_first_post',contains=None):
  assert cls.socket.parent==cls.root and cls.root.parent==pathlib.Path('/tmp')
  p=subprocess.run([str(PG/'psql'),'-XAtq','-v','ON_ERROR_STOP=1','-h',str(cls.socket),'-p','55487','-U','supa_pilot_admin','-d',database],input=text,env=cls.env,capture_output=True,text=True,timeout=10)
  if deny:
   if p.returncode==0:raise AssertionError('SQL unexpectedly authorized')
   if contains is not None and contains not in p.stderr:raise AssertionError('SQL failed outside intended security guard: '+p.stderr[-800:])
   return ''
  if p.returncode:raise AssertionError('disposable SQL failed: '+p.stderr[-800:])
  return p.stdout.strip()
 @classmethod
 def tearDownClass(cls):cls.command('pg_ctl','-D',str(cls.data),'-m','fast','-w','stop');cls.tmp.cleanup()
 def create_fixture(self,database,owner,agent):
  assert database=='supa_x_first_post' or (database.startswith('supa_x_pilot_check_') and database.removeprefix('supa_x_pilot_check_').isalnum())
  self.command('createdb','-h',str(self.socket),'-p','55487','-U','supa_pilot_admin',database)
  self.created_databases.append(database)  # Only databases this test created.
  for name in ['x-posting-foundation.sql','ecosystem-social.sql','social-review.sql','social-autonomy.sql','social-first-post.sql','x-worker-mock.sql','social-pilot-post.sql']:
   text=(ROOT/'deployment'/name).read_text()
   if name=='x-worker-mock.sql':
    assert text.count('CREATE ROLE supa_x_post_runtime LOGIN;')==1
    text=text.replace('CREATE ROLE supa_x_post_runtime LOGIN;','-- Existing cluster-local restricted fixture role.')
    text=text.replace('ON DATABASE supa_x_first_post', 'ON DATABASE '+database)
    text=text.replace('00000000-0000-0000-0000-000000000001',owner).replace('00000000-0000-0000-0000-00000000000a',agent)
   self.sql(text,database=database)
  self.sql("GRANT EXECUTE ON FUNCTION social_review.pilot_post(uuid,text,jsonb),social_review.pilot_cluster() TO supa_x_post_runtime; UPDATE social_review.policies SET account=jsonb_set(account,'{mode}','\"live\"');",database=database)
 def cleanup_databases(self):
  # No row deletion or trigger bypass: dispose only these newly created DBs.
  for database in reversed(self.created_databases):
   self.command('dropdb','-h',str(self.socket),'-p','55487','-U','supa_pilot_admin',database)
 def setUp(self):
  self.created_databases=[];self.addCleanup(self.cleanup_databases)
  namespace=uuid.uuid5(uuid.NAMESPACE_URL,str(self.root)+self.id())
  self.owner=str(uuid.uuid5(namespace,'owner'));self.agent=str(uuid.uuid5(namespace,'agent'))
  self.create_fixture('supa_x_first_post',self.owner,self.agent)
 def audit_snapshot(self):
  return self.sql("SELECT jsonb_build_object('count',count(*),'hash',encode(sha256(convert_to(coalesce(jsonb_agg(to_jsonb(e) ORDER BY id),'[]'::jsonb)::text,'UTF8')),'hex')) FROM social_autonomy.events e;")
 def assert_audit_integrity(self):
  self.assertEqual(self.sql("SELECT count(*) FROM (SELECT *,coalesce(lag(sha256) OVER(PARTITION BY agent_id ORDER BY id),repeat('0',64)) expected FROM social_autonomy.events) e WHERE previous_sha256<>expected OR sha256<>encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex');"),'0')
 def test_fixture_isolation_and_append_only_history(self):
  b=self.body();self.provision(b);self.call('reserve',b)
  self.assertEqual(self.sql('SELECT count(*) FROM social_review.attempts'),'1')
  before=self.audit_snapshot();self.assertGreater(json.loads(before)['count'],0)
  for sql in ['TRUNCATE social_autonomy.events;', 'DELETE FROM social_autonomy.events;', "UPDATE social_autonomy.events SET sha256=repeat('0',64);"]:
   self.sql(sql,True,contains='ecosystem_social_history_immutable')
   self.assertEqual(self.audit_snapshot(),before)
  second='supa_x_pilot_check_'+uuid.uuid4().hex;other_owner=str(uuid.uuid4());other_agent=str(uuid.uuid4())
  self.create_fixture(second,other_owner,other_agent)
  for table in ['social_review.attempts','social_review.drafts','social_review.pilot_approvals','social_autonomy.events']:
   self.assertEqual(self.sql('SELECT count(*) FROM '+table,database=second),'0')
  self.assertNotEqual(self.owner,other_owner)
  self.assertEqual(self.sql('SELECT owner_id::text FROM social_review.policies',database=second),other_owner)
  self.assertEqual(self.sql('SELECT owner_id::text FROM social_review.policies'),self.owner)
  self.assertEqual(self.audit_snapshot(),before);self.assert_audit_integrity()
 def body(self,text='Synthetic exact text pilot fixture.'):
  expiry=self.sql("SELECT (clock_timestamp()+interval '5 minutes')::text")
  authorization=str(uuid.uuid4())
  return {'authorization_id':authorization,'task_id':str(uuid.uuid4()),'receipt_sha256':hashlib.sha256(authorization.encode()).hexdigest(),'account':{'id':'1','username':'supa_agency','mode':'live'},'text':text,'text_sha256':hashlib.sha256(text.encode()).hexdigest(),'expires_at':expiry,'budget_microusd':25000,'confirmed':True,'read_write_confirmed':True,'credits_confirmed':True,'identity_receipt_sha256':'b'*64}
 def provision(self,b):
  q=lambda x:"'"+str(x).replace("'","''")+"'"
  self.sql('INSERT INTO social_review.pilot_approvals(authorization_id,task_id,owner_id,account,text_sha256,receipt_sha256,expires_at) VALUES('+','.join([q(b['authorization_id']),q(b['task_id']),q(self.owner),q(json.dumps(b['account']))+'::jsonb',q(b['text_sha256']),q(b['receipt_sha256']),q(b['expires_at'])])+');')
 def call(self,op,b,deny=False):
  raw=json.dumps(b).replace("'","''");s=self.sql(f"SET ROLE supa_x_post_runtime; SELECT social_review.pilot_post('{self.owner}','{op}','{raw}'::jsonb);",deny);return json.loads(s) if s else None
 def test_approval_cannot_be_created_by_runtime_or_configuration(self):
  b=self.body();self.call('reserve',b,True);self.sql('SET ROLE supa_x_post_runtime; INSERT INTO social_review.pilot_approvals DEFAULT VALUES;',True,contains='permission denied');self.assertEqual(self.sql('SELECT count(*) FROM social_review.attempts'),'0')
 def test_exact_hash_account_expiry_and_spend_are_enforced(self):
  b=self.body();self.provision(b)
  for key,value in [('text_sha256','c'*64),('account',{'id':'2','username':'supa_agency','mode':'live'}),('budget_microusd',25001),('task_id',str(uuid.uuid4()))]:
   bad=dict(b,**{key:value});self.call('reserve',bad,True)
  self.sql("UPDATE social_review.pilot_approvals SET expires_at=clock_timestamp()-interval '1 second'");self.call('reserve',b,True)
 def test_historical_variants_rejected(self):
  text='Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip.'
  for variant in [text,text.upper(),text.replace(' ','  '),text.replace('.','!')]:
   b=self.body(variant);self.provision(b);self.call('reserve',b,True)
 def test_claim_is_once_and_uncertainty_survives_restart(self):
  b=self.body();self.provision(b);self.assertTrue(self.call('reserve',b)['fresh']);self.assertFalse(self.call('reserve',b)['fresh']);self.assertTrue(self.call('peek',b));self.assertTrue(self.call('claim',b));self.call('claim',b,True)
  self.command('pg_ctl','-D',str(self.data),'-m','fast','-w','restart','-l',str(self.root/'postgres.log'))
  self.assertEqual(self.call('recover',b)['state'],'uncertain');self.call('peek',b,True)
  other=dict(b,authorization_id=str(uuid.uuid4()),task_id=str(uuid.uuid4()),receipt_sha256='c'*64);self.provision(other);self.call('reserve',other,True);self.assert_audit_integrity()
 def test_success_settlement_and_duplicate_content(self):
  b=self.body();self.provision(b);self.call('reserve',b);self.call('claim',b)
  self.call('settle',dict(b,receipt={'mode':'live','state':'posted','category':'verified_post_receipt','post_id':'123456','http_status':201,'rate_limit_reset':None}))
  self.assertEqual(self.call('read',b)['state'],'posted');self.assertFalse(self.call('reserve',b)['fresh'])
  other=dict(b,authorization_id=str(uuid.uuid4()),task_id=str(uuid.uuid4()),receipt_sha256='d'*64,text=b['text'].upper());other['text_sha256']=hashlib.sha256(other['text'].encode()).hexdigest();self.provision(other);self.call('reserve',other,True);self.assert_audit_integrity()
 def test_emergency_revocation_denies_claim_and_allows_quarantine(self):
  b=self.body();self.provision(b);self.call('reserve',b);self.sql('UPDATE social_review.pilot_approvals SET revoked=true');self.call('claim',b,True);self.assertEqual(self.call('recover',b)['state'],'uncertain');self.assert_audit_integrity()
if __name__=='__main__':unittest.main(failfast=True)
