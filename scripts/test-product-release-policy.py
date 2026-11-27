"""Synthetic exact-release policy checks; no network or project DB operations."""
import importlib.util,json,os,pathlib,subprocess,tempfile,unittest
from unittest.mock import patch
HERE=pathlib.Path(__file__).resolve().parent
TOOLS=HERE if (HERE/'scripts/git-policy.py').exists() else HERE.parent

def load(name,p):
 s=importlib.util.spec_from_file_location(name,p);m=importlib.util.module_from_spec(s);s.loader.exec_module(m);return m
P=load('policy',TOOLS/'scripts/git-policy.py')
C=load('creation',HERE/'create-product-checkout.py')
class PolicyTests(unittest.TestCase):
 def test_exact_product_branch_allowed(self):
  with patch.object(P,'product_context',return_value=True):P.validate(P.PRODUCT_BRANCH,[b'one.py'],'fix(product): preserve supervised task consent')
 def test_unrelated_branches_and_main_rejected(self):
  with patch.object(P,'product_context',return_value=True):
   for b in ('main','unrelated','release/x-pilot-validation-20261121'):
    with self.assertRaises(ValueError):P.validate(b,[b'one'])
 def test_other_contexts_retain_main_only(self):
  with patch.object(P,'product_context',return_value=False):
   P.validate('main',[b'one'])
   with self.assertRaises(ValueError):P.validate(P.PRODUCT_BRANCH,[b'one'])
 def test_one_file_conventional_rules(self):
  with patch.object(P,'product_context',return_value=True):
   for paths,msg in (([],None),([b'a',b'b'],None),([b'a'],'bad message')):
    with self.assertRaises(ValueError):P.validate(P.PRODUCT_BRANCH,paths,msg)
 def test_only_exact_matching_refspec(self):
  with patch.object(P,'product_context',return_value=True):
   P.validate_refspec(P.PRODUCT_REFSPEC)
   for ref in (P.MAIN_REFSPEC,P.RELEASE_REFSPEC,'refs/heads/a:refs/heads/b',':refs/heads/main'):
    with self.assertRaises(ValueError):P.validate_refspec(ref)
 def test_new_branch_update_allowed(self):
  ref='refs/heads/'+P.PRODUCT_BRANCH;head='a'*40
  with patch.object(P,'product_context',return_value=True):
   self.assertEqual(P.validate_push_updates([f'{ref} {head} {ref} '+P.ZERO_SHA],head,P.PRODUCT_REFSPEC),P.ZERO_SHA)
 def test_existing_branch_updates_and_deletion_rejected(self):
  ref='refs/heads/'+P.PRODUCT_BRANCH;head='a'*40
  with patch.object(P,'product_context',return_value=True):
   for old,new in [('b'*40,head),(P.ZERO_SHA,P.ZERO_SHA)]:
    with self.assertRaises(ValueError):P.validate_push_updates([f'{ref} {new} {ref} {old}'],new,P.PRODUCT_REFSPEC)
 def test_multiple_or_mismatched_updates_rejected(self):
  ref='refs/heads/'+P.PRODUCT_BRANCH;head='a'*40;line=f'{ref} {head} {ref} '+P.ZERO_SHA
  with patch.object(P,'product_context',return_value=True):
   for lines in ([],[line,line],[line.replace(ref,'refs/heads/main')],[line.replace(head,'c'*40)]):
    with self.assertRaises(ValueError):P.validate_push_updates(lines,head,P.PRODUCT_REFSPEC)
 def test_existing_destination_rejected(self):
  with tempfile.TemporaryDirectory() as t,patch.object(C,'DEST',pathlib.Path(t)):
   with self.assertRaisesRegex(ValueError,'destination_exists'):C.target_gate(pathlib.Path(t),C.BASE,C.TREE)
 def test_wrong_destination_and_baseline_rejected(self):
  with tempfile.TemporaryDirectory() as t:
   dest=pathlib.Path(t)/'new'
   with patch.object(C,'DEST',dest):
    C.target_gate(dest,C.BASE,C.TREE)
    for path,base,tree in ((dest.parent/'other',C.BASE,C.TREE),(dest,'0'*40,C.TREE),(dest,C.BASE,'0'*40)):
     with self.assertRaises(ValueError):C.target_gate(path,base,tree)
 def test_symlink_destination_rejected(self):
  with tempfile.TemporaryDirectory() as t:
   p=pathlib.Path(t)/'link';p.symlink_to(pathlib.Path(t)/'absent')
   with patch.object(C,'DEST',p):
    with self.assertRaises(ValueError):C.target_gate(p,C.BASE,C.TREE)
 def test_receipt_gate_names_complete(self):
  self.assertTrue({'rust','frontend','browser_desktop','browser_mobile','secrets','dependencies','history','consent_denial','staging_compatibility'}.issubset(P.PRODUCT_GATES))
 def test_wrapper_rejects_force_tags_mismatch_and_main_without_git(self):
  with tempfile.TemporaryDirectory() as t:
   root=pathlib.Path(t)/'candidate';(root/'scripts').mkdir(parents=True)
   wrapper=(TOOLS/'scripts/git-local').read_text().replace(str(P.PRODUCT_ROOT),str(root))
   p=root/'scripts/git-local';p.write_text(wrapper)
   for args in (['push','--force','origin',P.PRODUCT_REFSPEC],['push','--no-follow-tags','origin',P.MAIN_REFSPEC],['push','--tags'],['push','--no-follow-tags','other',P.PRODUCT_REFSPEC]):
    r=subprocess.run(['/bin/bash',str(p),*args],capture_output=True)
    self.assertNotEqual(r.returncode,0);self.assertIn(b'exact new-branch push',r.stderr)
 def test_hook_identity_and_scans_remain_enforced(self):
  source=(TOOLS/'scripts/git-policy.py').read_text()
  self.assertIn("ROOT/'.githooks'",source)
  self.assertIn('scan-secrets.py',source)
  self.assertIn('product_identity()',source)
  self.assertIn('audit()',source)
  self.assertIn("product-release-validation.json",source)
  self.assertIn("--separate-git-dir=",(HERE/'create-product-checkout.py').read_text())
 def test_matching_wrapper_command_reaches_guarded_git(self):
  with tempfile.TemporaryDirectory() as t:
   root=pathlib.Path(t)/'candidate';(root/'scripts').mkdir(parents=True);binpath=pathlib.Path(t)/'bin';binpath.mkdir()
   (root/'scripts/git-policy.py').write_text('')
   (binpath/'python3').write_text('#!/bin/sh\nexit 0\n');(binpath/'python3').chmod(0o755)
   (binpath/'git').write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n');(binpath/'git').chmod(0o755)
   p=root/'scripts/git-local';p.write_text((TOOLS/'scripts/git-local').read_text().replace(str(P.PRODUCT_ROOT),str(root)))
   r=subprocess.run(['/bin/bash',str(p),'push','--no-follow-tags','origin',P.PRODUCT_REFSPEC],env={'PATH':str(binpath)+':/usr/bin:/bin'},capture_output=True)
   self.assertEqual(r.returncode,0,r.stderr)
   self.assertIn(P.PRODUCT_REFSPEC.encode(),r.stdout)
 def test_receipt_and_clean_identity_binding(self):
  with tempfile.TemporaryDirectory() as t:
   root=pathlib.Path(t);(root/'.git-recovery').mkdir();head='a'*40;tree='b'*40
   def git(*a):
    if a==('rev-parse','HEAD'):return head.encode()
    if a==('rev-parse','HEAD^{tree}'):return tree.encode()
    if a==('status','--porcelain'):return b''
    if a[:2]==('remote','get-url'):return (P.OFFICIAL_REMOTE+'\n').encode()
    raise AssertionError(a)
   with patch.object(P,'ROOT',root),patch.object(P,'product_identity'),patch.object(P,'git',side_effect=git),patch.dict(os.environ,{'SUPA_APPROVED_PUSH_HEAD':head}):
    with self.assertRaises(ValueError):P.product_push_check()
    receipt=root/'.git-recovery/product-release-validation.json'
    data={'head':head,'base':P.PRODUCT_BASE,'tree':tree,'checks':dict.fromkeys(P.PRODUCT_GATES,True)}
    receipt.write_text(json.dumps(data));receipt.chmod(0o600)
    self.assertEqual(P.product_push_check(),head)
    for key in ('head','base','tree'):
     wrong=dict(data);wrong[key]='0'*40;receipt.write_text(json.dumps(wrong))
     with self.assertRaises(ValueError):P.product_push_check()
    for key in P.PRODUCT_GATES:
     wrong=dict(data,checks=dict(data['checks'],**{key:False}));receipt.write_text(json.dumps(wrong))
     with self.assertRaises(ValueError):P.product_push_check()
    receipt.write_text(json.dumps(data));receipt.chmod(0o644)
    with self.assertRaises(ValueError):P.product_push_check()
 def test_missing_hook_changed_main_or_merge_is_rejected(self):
  def git(*a):
   values={('symbolic-ref','--short','HEAD'):P.PRODUCT_BRANCH,('rev-parse','main'):P.PRODUCT_BASE,('rev-parse',P.PRODUCT_BASE+'^{tree}'):P.PRODUCT_TREE,('rev-list','--merges',P.PRODUCT_BASE+'..HEAD'):'',('config','--get','core.hooksPath'):str(P.ROOT/'.githooks')}
   return values[a].encode()
  with patch.object(P,'git',side_effect=git),patch.object(P,'ancestor',return_value=True):P.product_identity()
  for selected,bad in ((('rev-parse','main'),b'wrong'),(('config','--get','core.hooksPath'),b'/elsewhere'),(('rev-list','--merges',P.PRODUCT_BASE+'..HEAD'),b'merge')):
   def changed(*a):return bad if a==selected else git(*a)
   with patch.object(P,'git',side_effect=changed),patch.object(P,'ancestor',return_value=True):
    with self.assertRaises(ValueError):P.product_identity()
if __name__=='__main__':unittest.main()
