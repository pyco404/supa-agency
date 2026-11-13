#!/usr/bin/env python3
"""Focused fixture regressions; no server, DB, dotenv or external request."""
import json
from pathlib import Path
import runpy
import unittest

M = runpy.run_path(str(Path(__file__).with_name('validate-bounded-autonomy.py')))


class FixtureTests(unittest.TestCase):
    def test_sql_null_does_not_hide_empty_or_invalid_json(self):
        decode = M['decode_sql_json']
        self.assertIsNone(decode(M['SQL_NULL']))
        self.assertEqual(decode('{"state":"paused"}'), {'state': 'paused'})
        self.assertEqual(decode('[]'), [])
        for invalid in ['', 'not-json', M['SQL_NULL'] + '\n{}']:
            with self.assertRaises(json.JSONDecodeError):
                decode(invalid)

    def test_inventory_key_order_only_is_ignored(self):
        decode = M['decode_inventory']
        baseline = decode('[{"version":"18","port":55483,"config":{"ssl":"on","port":"55483"}}]')
        self.assertEqual(baseline, decode('[{"config":{"port":"55483","ssl":"on"},"port":55483,"version":"18"}]'))
        self.assertNotEqual(baseline, decode('[{"version":"18","port":5432,"config":{"ssl":"on","port":"55483"}}]'))
        self.assertNotEqual(baseline, decode('[{"version":"18","port":55483}]'))
        self.assertNotEqual(baseline, decode('[]'))
        for invalid in ['null', '{}', '[null]']:
            with self.assertRaises(ValueError):
                decode(invalid)


if __name__ == '__main__':
    unittest.main()
