#!/usr/bin/env python3
"""Deterministic sanitization and private-stream regressions; no live connection."""
import importlib.util
import json
from pathlib import Path
import unittest

s = importlib.util.spec_from_file_location('diagnostics', Path(__file__).with_name('recovery-export-diagnostics.py'))
m = importlib.util.module_from_spec(s)
s.loader.exec_module(m)


class DiagnosticsTests(unittest.TestCase):
    def test_client_errors_from_either_transport_channel(self):
        for channel in ['stdout', 'stderr']:
            data = {channel: 'DUMP_EXIT_CODE=1\nDUMP_CATEGORY=permission_denied\nFINAL_STAGE=single_export\n'}
            result = m.protocol(data.get('stdout', '') + '\n' + data.get('stderr', ''))
            self.assertEqual(result['dump_exit_code'], ['1'])
            self.assertEqual(result['dump_category'], ['permission_denied'])

    def test_private_values_never_returned(self):
        fixture_value = ''.join(['Private', 'Fixture', 'Password', '_4930'])
        url = ''.join(['postgresql://', 'owner', ':', fixture_value, '@private.invalid/db'])
        text = 'pg_dump: error: connection to server failed ' + 'password=' + fixture_value + ' ' + url + " /private/backup.dump\nquery was: COPY (SELECT 'sensitive row') TO STDOUT"
        result = json.dumps(m.diagnose(text, 'pg_dump', 1))
        for value in (fixture_value, 'postgresql://', 'private.invalid', '/private/', 'sensitive row', 'COPY'):
            self.assertNotIn(value, result)
        self.assertIn('connection_failed', result)
        self.assertIn('query_failed', result)

    def test_version_snapshot_tls_and_file_failures(self):
        for text, category in [('aborting because of server version mismatch', 'client_server_version_mismatch'),
                               ('invalid snapshot identifier', 'invalid_snapshot'),
                               ('FATAL: too many connections for role private_user', 'role_connection_limit'),
                               ('certificate verify failed', 'tls_verification_failed'),
                               ('File size limit exceeded', 'file_size_limit'),
                               ('timeout: failed to run command: No such file or directory', 'executable_unavailable')]:
            self.assertIn(category, m.diagnose(text, 'pg_dump', 1)['categories'])

    def test_unknown_and_timeout_fail_closed(self):
        self.assertEqual(m.diagnose('unrecognized private content', 'pg_dump', 1)['categories'], ['unknown_client_error'])
        self.assertEqual(m.diagnose('', 'pg_dump', 124)['exit_status'], 124)
        self.assertFalse(m.diagnose('done', 'pg_dump', 0)['raw_output_retained'])

    def test_bounded_and_invalid_protocol(self):
        self.assertNotIn('dump_category', m.protocol('DUMP_CATEGORY=postgresql://secret@host\n'))
        self.assertEqual(m.protocol('DUMP_CATEGORY=PrivateFixturePassword\nFINAL_STAGE=PrivateFixturePassword\n'), {})
        for operation, status in [('unapproved', 1), ('pg_dump', 256)]:
            with self.assertRaises(ValueError):
                m.diagnose('', operation, status)
        self.assertLess(len(json.dumps(m.diagnose('password=private' * 100000, 'pg_dump', 1))), 1000)


if __name__ == '__main__':
    unittest.main()
