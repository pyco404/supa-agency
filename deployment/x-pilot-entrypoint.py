#!/usr/bin/env python3
"""Default-paused manual target; reuses supervisor, never invokes its scheduler."""
import importlib.util
import json
import os
from pathlib import Path
import resource
import signal
import subprocess
import sys
import time
import uuid

spec = importlib.util.spec_from_file_location('x_supervisor', Path(__file__).with_name('entrypoint.py') if Path('/opt/supa-x/entrypoint.py').exists() else Path(__file__).with_name('x-worker-entrypoint.py'))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)

class PilotWorker(base.Worker):
    def __init__(self, root=base.ROOT, pg=base.PG, app=base.APP, environ=None):
        env = dict(os.environ if environ is None else environ)
        self.enabled = env.get('SUPA_X_PILOT_ENABLED', 'false')
        if self.enabled not in ('true', 'false') or env.get('SUPA_X_WORKER_MODE', 'paused') != 'paused':
            raise base.Denied('explicit_manual_pilot_required')
        self.receipt_id = env.get('SUPA_X_PILOT_RECEIPT', '')
        if self.enabled == 'true':
            try:
                if uuid.UUID(self.receipt_id).int == 0: raise ValueError()
            except ValueError:
                raise base.Denied('protected_pilot_receipt_required') from None
        # Base mode stays paused: it cannot resume the old scheduler on boot.
        super().__init__(root, pg, app, dict(env, SUPA_X_WORKER_MODE='paused'))
        if self.kind != 'live' or not (app/'bin/supa-x-pilot').is_file() or any((app/'bin'/name).exists() for name in ('supa-x-autonomy', 'supa-x-mock-worker')):
            raise base.Denied('pilot_bundle_required')

    def binary_args(self, operation):
        raise base.Denied('scheduler_operation_forbidden')

    def control_operation(self, operation):
        if operation not in ('pause', 'stop'):
            raise base.Denied('manual_authority_cannot_be_configured')
        base.write(self.control, {'active': False})

    def run(self):
        self.acquire()
        try:
            # Do not recover autonomous grants using a manual-only executable.
            if base.read(self.manifest).get('grant_ids') != []:
                raise base.Denied('manual_import_requires_no_autonomous_grants')
            self.prepare()
            self.serve_health()
            if self.enabled != 'true':
                while not self.stopping:
                    if not self.database_ready(): raise base.Denied('database_unavailable')
                    self.health={'state':'paused','database_ready':True}
                    self.heartbeat()
                    time.sleep(0.2)
                return
            marker=self.root/('pilot-consumed-'+str(uuid.UUID(self.receipt_id))+'.json')
            if marker.exists() or marker.is_symlink():
                raise base.Denied('pilot_start_already_consumed_no_retry')
            # Persistent launch intent precedes the child. A crash cannot auto-relaunch.
            base.write(marker, {'consumed':True})
            base.write(self.control, {'active':True})
            self.worker=subprocess.Popen([str(self.app/'bin/supa-x-pilot'),'run-once',self.receipt_id],env=self.child_env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True,pass_fds=(self.lock,))
            deadline=time.monotonic()+20
            while self.worker.poll() is None:
                if self.stopping or base.read(self.control).get('active') is not True or time.monotonic()>=deadline or not self.database_ready():
                    self.stop_child()
                    base.write(self.control, {'active':False,'reason':'unconfirmed'})
                    raise base.Denied('pilot_interrupted_no_retry')
                self.health={'state':'manual_one_shot','database_ready':True}
                self.heartbeat()
                time.sleep(0.1)
            code=self.worker.returncode
            self.worker=None
            base.write(self.control, {'active':False})
            if code != 0: raise base.Denied('pilot_child_unconfirmed_no_retry')
            # One outcome only. Shut down PostgreSQL and exit; never enter observe().
        finally:
            self.shutdown()

def main():
    os.umask(0o077)
    resource.setrlimit(resource.RLIMIT_CORE, (0,0))
    worker=PilotWorker()
    args=sys.argv[1:]
    if args:
        if len(args)!=2 or args[0]!='control':raise base.Denied('invalid_manual_arguments')
        worker.control_operation(args[1]);return
    def stop(*_):worker.stopping=True
    signal.signal(signal.SIGTERM,stop);signal.signal(signal.SIGINT,stop)
    worker.run()

if __name__=='__main__':
    try:main()
    except (base.Denied,OSError,ValueError,KeyError,subprocess.SubprocessError):
        print(json.dumps({'state':'pilot_stopped_unconfirmed','automatic_retry':False}),file=sys.stderr)
        sys.exit(1)
