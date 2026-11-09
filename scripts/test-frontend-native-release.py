#!/usr/bin/env python3
"""Local release-policy checks; no Railway mutations, builds or provider calls."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('secrets_gate', ROOT/'scripts/scan-secrets.py')
scan = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scan)

class FrontendReleaseTests(unittest.TestCase):
    def fixture(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        for name in ('web/src', 'web/public', 'reference-web', 'web/.next/static'):
            (root/name).mkdir(parents=True)
        return root

    def test_source_and_built_public_chunks_are_scanned(self):
        root=self.fixture();(root/'web/src/page.ts').write_text('export const publicTitle="Supa Agency";')
        (root/'web/.next/static/chunk.js').write_text('console.log("public shell")')
        result=scan.frontend_context(root)
        self.assertTrue(result['passed']);self.assertEqual(result['files_scanned'],2)

    def test_client_secret_is_rejected_without_disclosing_value(self):
        root=self.fixture(); secret='ghp_'+'aB9fP4sT7mN2kR8vL5xD0qW6zY1cE3uI9oS4'
        (root/'web/.next/static/chunk.js').write_text('const credential="'+secret+'";')
        result=scan.frontend_context(root)
        self.assertFalse(result['passed']);self.assertNotIn(secret,json.dumps(result))

    def test_private_dotenv_rejected_without_reading_contents(self):
        root=self.fixture();(root/'.env').write_text('password=private-do-not-echo')
        result=scan.frontend_context(root)
        self.assertFalse(result['passed']);self.assertNotIn('private-do-not-echo',json.dumps(result))

    def test_missing_input_and_symlink_fail_closed(self):
        root=self.fixture();(root/'web/src/link').symlink_to('/etc/passwd')
        with self.assertRaises(RuntimeError):scan.frontend_context(root)
        (root/'web/src/link').unlink();(root/'web/.next/static').rmdir()
        with self.assertRaises(RuntimeError):scan.frontend_context(root)

    def test_file_bounds_fail_closed(self):
        root=self.fixture();p=root/'web/public/oversize';p.touch()
        with p.open('wb') as f:f.truncate(64*1024*1024+1)
        with self.assertRaises(RuntimeError):scan.frontend_context(root)

    def test_config_changes_only_frontend_build_and_health(self):
        config=tomllib.loads((ROOT/'deployment/frontend.railway.toml').read_text())
        self.assertEqual(set(config),{'build','deploy'})
        self.assertEqual(config['build']['dockerfilePath'],'deployment/frontend-railway.Dockerfile')
        self.assertEqual(config['deploy']['healthcheckPath'],'/healthz')
        self.assertEqual(config['deploy']['healthcheckTimeout'],120)
        self.assertNotIn('preDeployCommand',config['deploy'])
        self.assertTrue(all(not any(p in pattern for p in ('crates','migrations','api.Dockerfile')) for pattern in config['build']['watchPatterns']))

    def test_reviewed_runtime_and_single_build_preserved(self):
        recipe=(ROOT/'deployment/frontend-railway.Dockerfile').read_text()
        base='node:26.13.0-alpine3.23@sha256:cda001211d4b778be8c44dce67cd65ae14619baba11b9217da91bc81d337c91d'
        self.assertEqual(recipe.count('FROM '+base+' AS '),2)
        self.assertEqual(recipe.count('npm run build'),1)
        for gate in ('npm run typecheck','npm run lint','--frontend-context /app','timeout -s TERM 180 node','npm audit --omit=dev --audit-level=high','USER 65532:65532'):
            self.assertIn(gate,recipe)
        for page in ('my-agents.html','factory-create.html','public-agent-profile.html'):
            self.assertIn('COPY reference-web/'+page+' /app/reference-web/',recipe)
        self.assertNotIn('ARG ',recipe)
        self.assertNotIn('SUPA_ALLOW_OPENAI=true',recipe)

    def test_frontend_context_default_deny_excludes_credentials(self):
        lines=(ROOT/'deployment/frontend-railway.Dockerfile.dockerignore').read_text().splitlines()
        self.assertEqual(next(l for l in lines if l and not l.startswith('#')),'**')
        for rule in ('**/.env*','**/*.pem','**/*.key','**/node_modules','**/.next','**/.git'):
            self.assertIn(rule,lines)
        self.assertNotIn('!crates/**',lines)
        self.assertNotIn('!migrations/**',lines)

    def test_original_scanner_still_passes_self_tests(self):
        result=subprocess.run(['python3',str(ROOT/'scripts/scan-secrets.py'),'--self-test'],capture_output=True,text=True,timeout=20)
        self.assertEqual(result.returncode,0)

if __name__=='__main__':unittest.main()
