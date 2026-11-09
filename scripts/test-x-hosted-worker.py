#!/usr/bin/env python3
"""Offline contracts only: no credentials, live databases, X requests or services."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[1]
BINARY=ROOT/'target/work-review-validation/debug/supa-x-autonomy'
UNIT=ROOT/'deployment/systemd/supa-x-autonomy.service'
class HostedWorker(unittest.TestCase):
    def test_invalid_or_uninstalled_authority_fails_before_configuration(self):
        for args in [[],['run'],['run','not-a-uuid'],['run','00000000-0000-0000-0000-000000000000'],['check','11111111-1111-4111-8111-111111111111'],['authorize','11111111-1111-4111-8111-111111111111'],['recover-paused','00000000-0000-0000-0000-000000000000'],['recover-paused','11111111-1111-4111-8111-111111111111'],['run','x','--enable']]:
            r=subprocess.run([str(BINARY),*args],env={'PATH':'/usr/bin:/bin'},capture_output=True,text=True,timeout=5)
            self.assertEqual(r.returncode,1)
            self.assertEqual(r.stdout,'')
            self.assertEqual(r.stderr,'STOPPED: X worker unconfirmed; no automatic dispatch retry. Inspect durable state privately.\n')
    def test_unit_preserves_bounds_and_never_auto_authorizes_or_resumes(self):
        text=UNIT.read_text()
        for setting in ['User=supa-x','LoadCredentialEncrypted=oauth1.json:', 'Restart=on-failure','RuntimeMaxSec=24h','NoNewPrivileges=true','ProtectSystem=strict','ProtectHome=true','LimitCORE=0','MemoryMax=256M']:
            self.assertIn(setting,text)
        commands=[x for x in text.splitlines() if x.startswith('ExecStart=')]
        self.assertEqual(len(commands),1);self.assertIn(' run ',commands[0])
        self.assertNotIn('authorize',commands[0]);self.assertNotIn('resume',commands[0])
        self.assertIn('ExecReload=',text)
    def test_systemd_parser_without_installing_a_service(self):
        text=UNIT.read_text()
        # Only replace the not-yet-installed executable for the static parser.
        text=text.replace('/opt/supa-agent/bin/supa-x-autonomy','/bin/true')
        with tempfile.TemporaryDirectory(prefix='supa-x-unit-') as folder:
            path=Path(folder)/UNIT.name;path.write_text(text)
            r=subprocess.run(['systemd-analyze','verify','--man=no',str(path)],capture_output=True,text=True,timeout=15)
            self.assertEqual(r.returncode,0,r.stderr)
if __name__=='__main__':unittest.main()
