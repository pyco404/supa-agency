#!/usr/bin/env python3
"""Offline supervisor contracts; no real accounts, credentials or database."""
import importlib.util,json,os,pathlib,signal,subprocess,tempfile,threading,time,unittest,uuid
from unittest.mock import patch
ROOT=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('pilot',ROOT/'deployment/x-pilot-entrypoint.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class Pilot(unittest.TestCase):
 def worker(self,folder,enabled='false',code='raise SystemExit(0)'):
  root=pathlib.Path(folder)/'volume';root.mkdir(mode=0o700,exist_ok=True);app=pathlib.Path(folder)/'app';(app/'bin').mkdir(parents=True,exist_ok=True);(app/'bundle-mode').write_text('live')
  binary=app/'bin/supa-x-pilot';binary.write_text('#!/usr/bin/python3\n'+code+'\n');binary.chmod(0o700)
  m.base.write(root/'ledger.json',{'version':1,'mode':'live','import_verified':True,'grant_ids':[]})
  return m.PilotWorker(root=root,pg=pathlib.Path('/not-used'),app=app,environ={'SUPA_X_PILOT_ENABLED':enabled,'SUPA_X_PILOT_RECEIPT':str(uuid.UUID(int=1))})
 def execute(self,w):
  with patch.object(w,'prepare'),patch.object(w,'serve_health'),patch.object(w,'database_ready',return_value=True):w.run()
 def test_default_disabled_and_scheduler_excluded(self):
  with tempfile.TemporaryDirectory() as f:
   w=self.worker(f)
   with self.assertRaises(m.base.Denied):w.binary_args('run')
   with self.assertRaises(m.base.Denied):w.control_operation('resume')
   with patch.object(w,'prepare'),patch.object(w,'serve_health'),patch.object(w,'database_ready',return_value=True),patch.object(m.time,'sleep',side_effect=lambda _:setattr(w,'stopping',True)),patch.object(m.subprocess,'Popen') as start:
    w.run();start.assert_not_called()
 def test_success_exits_once_and_restart_cannot_replay(self):
  with tempfile.TemporaryDirectory() as f:
   w=self.worker(f,'true');self.execute(w);self.assertFalse(m.base.read(w.control)['active']);self.assertIsNone(w.worker);self.assertIsNone(w.lock)
   again=self.worker(f,'true')
   with self.assertRaises(m.base.Denied):self.execute(again)
 def test_child_failure_is_consumed_no_retry(self):
  with tempfile.TemporaryDirectory() as f:
   w=self.worker(f,'true','raise SystemExit(1)')
   with self.assertRaises(m.base.Denied):self.execute(w)
   self.assertIsNone(w.worker);self.assertIsNone(w.lock)
   with self.assertRaises(m.base.Denied):self.execute(self.worker(f,'true'))
 def test_emergency_stop_terminates_child(self):
  with tempfile.TemporaryDirectory() as f:
   w=self.worker(f,'true','import time; time.sleep(30)')
   original=m.base.read
   def read(path):
    value=original(path)
    if path==w.control and value.get('active'):return {'active':False}
    return value
   with patch.object(m.base,'read',side_effect=read):
    with self.assertRaises(m.base.Denied):self.execute(w)
   self.assertIsNone(w.worker);self.assertFalse(m.base.read(w.control)['active'])
 def test_missing_receipt_and_wrong_executable_rejected(self):
  with tempfile.TemporaryDirectory() as f:
   w=self.worker(f)
   with self.assertRaises(m.base.Denied):m.PilotWorker(w.root,w.pg,w.app,{'SUPA_X_PILOT_ENABLED':'true'})
   (w.app/'bin/supa-x-autonomy').write_text('not allowed')
   with self.assertRaises(m.base.Denied):m.PilotWorker(w.root,w.pg,w.app,{})
 def test_autonomous_import_rejected_before_prepare(self):
  with tempfile.TemporaryDirectory() as f:
   w=self.worker(f,'true');m.base.write(w.manifest,{'grant_ids':[str(uuid.UUID(int=2))]})
   with patch.object(w,'prepare') as prep:
    with self.assertRaises(m.base.Denied):w.run()
    prep.assert_not_called()
if __name__=='__main__':unittest.main(failfast=True)
