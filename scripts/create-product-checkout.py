"""Approved create-only product checkout bootstrap; root checkout stays unchanged."""
import hashlib,json,os,pathlib,subprocess,sys
ROOT=pathlib.Path('/home/pikoo/Desktop/supa-agent')
OUT=ROOT/'artifacts/phase8-product-release-20261127'
TOOLS=OUT/'tooling'
DEST=OUT/'application-release'
BASE='977ffa48417015a64c2b9c5f32b438c3f131f20f'
TREE='e8112c94136b79ffc91c71b2fa9f8c507f3d6e07'
BRANCH='release/product-staging-20261127'
REMOTE='https://github.com/pyco404/supa-agency'
FILES=('scripts/git-local','scripts/git-policy.py','scripts/commit-one')
def require(ok,reason):
 if not ok:raise ValueError(reason)
def run(args,cwd):
 env={k:os.environ[k] for k in ('PATH','HOME','LANG') if k in os.environ}
 env.update(GIT_TERMINAL_PROMPT='0',GIT_CONFIG_COUNT='0')
 r=subprocess.run(args,cwd=cwd,env=env,capture_output=True,timeout=120)
 require(r.returncode==0,'bootstrap_command_failed')
 return r.stdout
def git(repo,*args):
 return run(['/usr/bin/git','--git-dir='+str(repo/'.git-recovery'),'--work-tree='+str(repo),*args],repo)
def original():
 return (git(ROOT,'rev-parse','HEAD'),git(ROOT,'for-each-ref'),git(ROOT,'status','--porcelain'),hashlib.sha256((ROOT/'.git-recovery/index').read_bytes()).hexdigest(),hashlib.sha256((ROOT/'web/next-env.d.ts').read_bytes()).hexdigest())
def target_gate(destination,base,tree):
 require(destination==DEST and destination.is_absolute(),'exact_destination_required')
 require(not destination.exists() and not destination.is_symlink(),'destination_exists')
 require(destination.parent.resolve()==destination.parent and destination.parent.is_dir(),'canonical_parent_required')
 require(base==BASE and tree==TREE,'pinned_base_and_tree_required')
def create():
 target_gate(DEST,BASE,TREE)
 receipt=json.loads((OUT/'tooling-validation.json').read_text())
 require(receipt.get('passed') is True and receipt.get('tests',0)>=12,'tooling_tests_required')
 for name in (*FILES,'create-product-checkout.py','test-product-release-policy.py'):
  p=TOOLS/name
  require(not p.is_symlink() and p.is_file() and hashlib.sha256(p.read_bytes()).hexdigest()==receipt['hashes'][name],'tested_tooling_hash_required')
 before=original()
 # Reviewed bootstrap exception: no checkout/commit before destination hooks exist.
 run(['/usr/bin/git','clone','--no-checkout','--no-tags','--single-branch','--branch','main','--template=','--separate-git-dir='+str(DEST/'.git-recovery'),REMOTE,str(DEST)],OUT)
 os.chmod(DEST,0o700)
 require(git(DEST,'rev-parse','HEAD').decode().strip()==BASE and git(DEST,'rev-parse','HEAD^{tree}').decode().strip()==TREE,'cloned_baseline_changed')
 git(DEST,'checkout','-b',BRANCH,BASE)
 git(DEST,'config','--local','core.hooksPath',str(DEST/'.githooks'))
 for name in FILES:
  p=DEST/name
  require(p.is_file() and not p.is_symlink(),'expected_tooling_file_required')
  p.write_bytes((TOOLS/name).read_bytes())
  os.chmod(p,0o755 if name in ('scripts/git-local','scripts/commit-one') else 0o644)
 for name in ('create-product-checkout.py','test-product-release-policy.py'):
  p=DEST/'scripts'/name
  require(not p.exists() and not p.is_symlink(),'new_tooling_path_required')
  p.write_bytes((TOOLS/name).read_bytes())
 for name in ('pre-commit','commit-msg','pre-push'):
  p=DEST/'.githooks'/name
  require(p.is_file() and not p.is_symlink() and os.access(p,os.X_OK),'checkout_hooks_required')
 require(git(DEST,'rev-parse','--absolute-git-dir').decode().strip()==str(DEST/'.git-recovery'),'separate_metadata_required')
 require(not (DEST/'.git-recovery/objects/info/alternates').exists(),'no_shared_object_alternates')
 require(git(DEST,'rev-parse','main').decode().strip()==BASE,'main_must_stay_pinned')
 require(not (DEST/'.env').exists() and not (DEST/'artifacts').exists(),'private_artifacts_excluded')
 require(original()==before,'root_checkout_changed')
 return {'passed':True,'branch':BRANCH,'base':BASE,'tree':TREE,'destination':str(DEST),'root_unchanged':True,'private_artifacts_copied':False,'hooks_local':True}
if __name__=='__main__':
 try:
  require(len(sys.argv)==2 and sys.argv[1]==str(DEST),'exact_destination_argument_required')
  print(json.dumps(create()))
 except Exception as e:
  print(json.dumps({'passed':False,'category':str(e) if isinstance(e,ValueError) else type(e).__name__,'retry':False}),file=sys.stderr)
  raise SystemExit(1)
