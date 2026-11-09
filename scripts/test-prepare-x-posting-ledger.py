#!/usr/bin/env python3
"""Pure setup contracts. No server, credentials or existing database is accessed."""
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('x_post_setup',ROOT/'scripts/prepare-x-posting-ledger.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

class Preparation(unittest.TestCase):
    def test_minimal_foundation_reuses_reviewed_definitions(self):
        foundation=(ROOT/'deployment/x-posting-foundation.sql').read_text()
        for name,tables in [('migrations/0001_foundation.sql',['agents','cycles','activity']),('migrations/0006_authorization_ownership_health.sql',['agent_access']),('migrations/0008_application_epoch.sql',['application_epoch'])]:
            text=(ROOT/name).read_text()
            for table in tables:
                start=text.index('CREATE TABLE '+table+' (');end=text.index('\n);',start)+3
                self.assertIn(text[start:end],foundation)
        self.assertEqual(foundation.count('CREATE TABLE '),5)
        self.assertNotIn('CREATE TABLE wallet',foundation)
        self.assertNotIn('CREATE TABLE simulated_ledger',foundation)
        self.assertNotIn('CREATE TABLE live_budget',foundation)
        text=(ROOT/'migrations/0013_work_evaluation_reviews.sql').read_text()
        start=text.index('CREATE FUNCTION work_review_canonical');end=text.index('$$;',start)+3
        self.assertIn(text[start:end],foundation)

    def test_diagnostics_retain_static_category_exit_and_operation_without_secret(self):
        for message,category in [(b'undefined symbol: PQsendPipelineSync synthetic-secret','pg18_client_loaded_incompatible_libpq'),(b'ERROR: synthetic-secret','local_sql_rejected'),(b'authentication failed synthetic-secret','local_authentication_denied')]:
            m.DIAGNOSTIC.clear()
            with patch.object(m.subprocess,'run',return_value=subprocess.CompletedProcess(['psql'],1,b'',message)):
                with self.assertRaises(ValueError) as caught:m.run(['/private/psql'],{},b'synthetic-secret')
            self.assertEqual(m.DIAGNOSTIC['category'],category)
            self.assertEqual(m.DIAGNOSTIC['exit_code'],1)
            self.assertEqual(m.DIAGNOSTIC['operation'],'psql')
            self.assertNotIn('synthetic-secret',str(caught.exception)+str(m.DIAGNOSTIC))

if __name__=='__main__':unittest.main()
