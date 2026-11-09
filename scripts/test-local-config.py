#!/usr/bin/env python3
"""Configuration regressions: disposable files, synthetic keys, no network or database."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('local_config', ROOT / 'scripts/local-config.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class Configuration(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='supa-config-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.template = (ROOT / '.env.example').read_text()
        (self.root / '.env.example').write_text(self.template)
        self.path = self.root / '.env'
        self.path.write_text(self.template)
        self.path.chmod(0o600)
        self.values = m.parse(self.template)
        self.values.update(DATABASE_URL='postgresql://synthetic@127.0.0.1:55432/disposable',
                           SUPA_API_TOKEN='synthetic-token-for-local-tests-000000',
                           SUPA_WEB_USERNAME='synthetic', SUPA_WEB_PASSWORD='synthetic-password-for-tests-0000',
                           SUPA_X_CLIENT_ID='public-fixture', SUPA_X_OWNER_ID='00000000-0000-4000-8000-000000000001')

    def test_literal_loading_precedence_and_profiles(self):
        (self.root / '.env.eval').write_text('OPENAI_API_KEY=must-not-load')
        (self.root / '.env.devnet').write_text('SUPA_WALLET_ENABLED=true')
        loaded = m.load(self.root, {'PORT': '3300', 'SUPA_WEB_PASSWORD': ''})
        self.assertEqual(loaded['PORT'], '3300')
        self.assertEqual(loaded['SUPA_WEB_PASSWORD'], '')
        self.assertEqual(loaded['OPENAI_API_KEY'], '')
        self.assertEqual(loaded['SUPA_WALLET_ENABLED'], 'false')
        self.assertEqual(m.parse('OPENAI_API_KEY=$(literal)`literal`')['OPENAI_API_KEY'], '$(literal)`literal`')
        self.assertEqual(m.parse("SUPA_WEB_OPERATORS='[{\"role\":\"viewer\"}]'")['SUPA_WEB_OPERATORS'], '[{"role":"viewer"}]')

    def test_permissions_symlink_hardlink_and_missing(self):
        self.path.chmod(0o644)
        with self.assertRaises(m.Invalid): m.load(self.root, {})
        self.path.unlink()
        with self.assertRaises(OSError): m.load(self.root, {})
        self.path.symlink_to(self.root / '.env.example')
        with self.assertRaises(OSError): m.load(self.root, {})
        self.path.unlink()
        target = self.root / 'private'
        target.write_text(self.template)
        target.chmod(0o600)
        os.link(target, self.path)
        with self.assertRaises(m.Invalid): m.load(self.root, {})

    def test_syntax_unknowns_and_secret_redaction(self):
        for text in ['bad fixture-secret', 'OPENAI_API_KEY="fixture-secret',
                     'PORT=3200\nPORT=3300', 'PATH=fixture-secret',
                     'SUPA_X_CLIENT_SECRET=fixture-secret', 'NEXT_PUBLIC_TOKEN=fixture-secret']:
            self.path.write_text(text)
            with self.assertRaises(m.Invalid) as caught: m.load(self.root, {})
            self.assertNotIn('fixture-secret', str(caught.exception))
        for key in ['NEXT_PUBLIC_OPENAI_API_KEY', 'SUPA_X_ACCESS_TOKEN', 'SUPA_SITE_ENV_FILE']:
            with self.assertRaises(m.Invalid): m.load(self.root, {key: 'fixture-secret'})

    def test_valid_and_missing_fields(self):
        self.assertEqual(m.validate(self.values), [])
        for field in ['DATABASE_URL', 'SUPA_API_TOKEN', 'SUPA_WEB_PASSWORD', 'SUPA_X_CLIENT_ID']:
            values = dict(self.values, **{field: ''})
            self.assertIn(field, m.validate(values))
        self.assertEqual(m.validate(m.parse(self.template), 'mint'), [])

    def test_local_x_setup_does_not_require_or_grant_platform_owner_identity(self):
        values=dict(self.values, SUPA_X_OWNER_ID='')
        self.assertEqual(m.validate(values,'x'),[])
        self.assertIn('SUPA_X_OWNER_ID', m.validate(dict(values,SUPA_X_OWNER_ID='not-a-uuid'),'x'))
        self.assertEqual(values['SUPA_X_OWNER_ID'],'')

    def test_oauth1_configuration_requires_no_oauth2_client_or_platform_owner(self):
        values=dict(self.values,SUPA_X_CLIENT_ID='',SUPA_X_OWNER_ID='',
                    **{key:'synthetic-fixture-only' for key in m.OAUTH1_FIELDS})
        self.assertEqual(m.validate(values,'x-oauth1'),[])
        self.path.write_text('\n'.join(key+'='+value for key,value in values.items())+'\n')
        self.assertEqual(m.oauth1_credentials(self.root),{key:values[key] for key in m.OAUTH1_FIELDS})
        report=m.oauth1_status(values)
        self.assertTrue(report['configured'])
        self.assertNotIn('synthetic-fixture-only',json.dumps(report))
        for key in m.OAUTH1_FIELDS:
            for bad in ['', 'contains space', 'x'*1025]:
                invalid=dict(values,**{key:bad})
                self.assertFalse(m.oauth1_status(invalid)['configured'])
                self.assertIn(key,m.validate(invalid,'x-oauth1'))
        self.assertFalse(m.oauth1_status({})['configured'])
        self.assertEqual(m.load(self.root,{'X_API_KEY':'export-override'})['X_API_KEY'],'export-override')
        # Credential IPC always chooses the private file rather than exported overrides.
        with patch.dict(os.environ, {'X_API_KEY':'export-override'}):
            self.assertEqual(m.oauth1_credentials(self.root)['X_API_KEY'],'synthetic-fixture-only')

    def test_first_post_launcher_keeps_credentials_private_and_requires_exact_options(self):
        binary=self.root/'target/work-review-validation/debug/supa-x-first-post'
        binary.parent.mkdir(parents=True);binary.write_text('synthetic executable fixture')
        values=dict(self.values,**{key:'synthetic-x-fixture-only' for key in m.OAUTH1_FIELDS})
        class Launched(Exception): pass
        for action in ['x-first-post-prepare','x-first-post']:
            argv=['local-config.py',action]
            if action=='x-first-post':argv+=['--authorization-id','00000000-0000-4000-8000-000000000001','--expires-at','2000000000']
            with patch.object(m,'ROOT',self.root),patch.object(m,'load',return_value=values),patch.object(m.sys,'argv',argv),patch.object(m.os,'isatty',return_value=True),patch.object(m.os,'chdir'),patch.object(m.os,'execve',side_effect=Launched) as launch:
                with self.assertRaises(Launched):m.main()
                _,args,env=launch.call_args.args
                for key in m.OAUTH1_FIELDS:self.assertNotIn(key,env)
                if action=='x-first-post':
                    self.assertEqual(args[-1],'--authorize-one-get-and-one-post-budget-025')
                    self.assertIn('DATABASE_URL',env)
                else:self.assertEqual(args[-1],'prepare');self.assertNotIn('DATABASE_URL',env)
        with patch.object(m,'ROOT',self.root),patch.object(m,'load',return_value=values),patch.object(m.sys,'argv',['local-config.py','x-first-post']),patch.object(m.os,'isatty',return_value=True):
            with self.assertRaises(m.Invalid):m.main()

    def test_boundaries_disabled_capabilities_and_malformed(self):
        for key in m.DISABLED:
            for bad in ['true', '1', 'FALSE', '']:
                self.assertIn(key, m.validate(dict(self.values, **{key: bad})))
        for field, bad in [('DATABASE_URL', 'postgresql://synthetic@remote.invalid:5432/data'),
                           ('SUPA_SITE_MODE', 'production'), ('SUPA_SITE_ORIGIN', 'https://supaagency.xyz'),
                           ('SUPA_BACKEND_URL', 'http://127.0.0.1:8080/?secret=x'),
                           ('SUPA_BIND', '0.0.0.0:8080'), ('PORT', '70000'),
                           ('SUPA_X_CALLBACK_URL', 'http://localhost:37641/oauth/x/callback'),
                           ('SUPA_X_USERNAME', 'another'), ('SUPA_X_CREDENTIAL_CUSTODY', 'another'),
                           ('SUPA_PLATFORM_MINT', m.MINT.lower()), ('SUPA_PROVIDER', 'openai'),
                           ('SUPA_MAX_OUTPUT_TOKENS', '257'), ('SUPA_LIVE_BUDGET_MICROUSD', '1'),
                           ('SUPA_OPERATOR_SESSION_DEADLINES', '[]')]:
            self.assertIn(field, m.validate(dict(self.values, **{field: bad})))

    def test_process_isolation_and_commands(self):
        values = dict(self.values, **{key:'synthetic-x-fixture-only' for key in m.OAUTH1_FIELDS}, OPENAI_API_KEY='fixture-secret', SUPA_PLATFORM_MINT_KEYPAIR_PATH='/private/key')
        for component in ['web', 'preview', 'x', 'api']:
            argv, cwd, env = m.command(values, component, {'PATH': '/usr/bin', 'OPENAI_API_KEY': 'shell-secret',
                                                         'NODE_OPTIONS': '--inspect=0.0.0.0', 'DATABASE_URL': 'bad'})
            self.assertTrue(argv)
            for key in m.OAUTH1_FIELDS: self.assertNotIn(key, env)
            self.assertNotIn('NODE_OPTIONS', env)
            self.assertNotIn('OPENAI_API_KEY', env)
            self.assertNotIn('SUPA_PLATFORM_MINT_KEYPAIR_PATH', env)
            if component != 'api': self.assertNotIn('DATABASE_URL', env)
            if component == 'api': self.assertNotIn('PORT', env)
            if component == 'preview': self.assertEqual(env['SUPA_SITE_ENV_FILE'], '')
        self.assertNotIn('NEXT_PUBLIC_', (ROOT / 'web/next.config.ts').read_text())
        self.assertNotIn('env:', (ROOT / 'web/next.config.ts').read_text())

    def test_mint_seed_public_and_identity_validation(self):
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
        seed = bytes(range(32))  # Synthetic fixture, never a production key.
        public = Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
        number, address = int.from_bytes(public, 'big'), ''
        while number:
            number, digit = divmod(number, 58)
            address = alphabet[digit] + address
        keyfile = self.root / 'fixture.json'
        keyfile.write_text(json.dumps(list(seed + public)))
        keyfile.chmod(0o600)
        with patch.object(m, 'MINT', address):
            # Successful derivation is still rejected without required SUPA prefix.
            with self.assertRaises(m.Invalid):
                m.verify_mint(dict(self.values, SUPA_PLATFORM_MINT=address, SUPA_PLATFORM_MINT_KEYPAIR_PATH=str(keyfile)))
        for data in [list(seed + public[:-1] + b'\0'), [True] * 64, [0] * 32, '<script>']:
            keyfile.write_text(json.dumps(data))
            with self.assertRaises(m.Invalid):
                m.verify_mint(dict(self.values, SUPA_PLATFORM_MINT_KEYPAIR_PATH=str(keyfile)))

    def test_real_preview_startup_and_private_file_denial(self):
        import socket
        import time
        import urllib.request
        import urllib.error
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        values = dict(self.values, PORT=str(port))
        argv, cwd, env = m.command(values, 'preview', {'PATH': os.environ['PATH'], 'HOME': str(self.root)})
        process = subprocess.Popen(argv, cwd=cwd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            base = 'http://127.0.0.1:' + str(port)
            deadline = time.monotonic() + 10
            while True:
                try:
                    with urllib.request.urlopen(base + '/healthz', timeout=1) as response:
                        self.assertEqual(response.status, 200)
                    break
                except (OSError, urllib.error.URLError):
                    self.assertIsNone(process.poll())
                    if time.monotonic() >= deadline: self.fail('preview startup deadline')
                    time.sleep(0.05)
            for path in ['/', '/factory', '/agents', '/about']:
                with urllib.request.urlopen(base + path, timeout=2) as response:
                    self.assertEqual(response.status, 200)
            for path, status in [('/my-agents', 401), ('/operator', 401), ('/.env', 404), ('/.env.eval', 404)]:
                with self.assertRaises(urllib.error.HTTPError) as caught:
                    urllib.request.urlopen(base + path, timeout=2)
                self.assertEqual(caught.exception.code, status)
        finally:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)

    def test_git_ignore_and_eval_contract(self):
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True, capture_output=True)
        (self.root / '.gitignore').write_text((ROOT / '.gitignore').read_text())
        for name in ['.env', '.env.eval', '.env.staging', '.env.production', 'web/.env.local']:
            self.assertEqual(subprocess.run(['git', '-C', str(self.root), 'check-ignore', '-q', name]).returncode, 0)
        for name in ['.env.example', '.env.eval.example']:
            self.assertEqual(subprocess.run(['git', '-C', str(self.root), 'check-ignore', '-q', name]).returncode, 1)
        import re
        source = (ROOT / 'crates/runtime/src/evaluation.rs').read_text().split('];', 1)[0]
        allowed = set(re.findall(r'"([A-Z][A-Z0-9_]*)"', source))
        self.assertLessEqual(set(m.parse((ROOT / '.env.eval.example').read_text())), allowed)


if __name__ == '__main__':
    unittest.main()
