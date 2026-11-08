#!/usr/bin/env python3
"""Real CLI denial path through a synthetic private terminal. No consent or credentials."""
import os
from pathlib import Path
import pty
import resource
import select
import subprocess
import time
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/work-review-validation/debug/supa-x-oauth-local'
OWNER = '6fd863e1-745d-4a06-b68e-eea69e854a69'
FLAG = '--authorize-one-connection-and-one-budgeted-identity-read'


def terminal(args):
    master, slave = pty.openpty()
    def no_core(): resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    child = subprocess.Popen([str(BINARY), *args], stdin=slave, stdout=slave, stderr=slave,
        cwd=ROOT, env={'PATH': '/usr/bin', 'LANG': 'C.UTF-8'}, preexec_fn=no_core)
    os.close(slave)
    output, confirmed = bytearray(), False
    deadline = time.monotonic() + 10
    try:
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.05)
            if ready:
                try: chunk = os.read(master, 4096)
                except OSError: break
                if not chunk: break
                output.extend(chunk)
                if b'Type CONNECT' in output and not confirmed:
                    os.write(master, b'CONNECT @supa_agency ONCE\n')
                    confirmed = True
            if child.poll() is not None and not ready: break
        code = child.wait(timeout=1)
        return code, bytes(output), confirmed
    finally:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=2)
        os.close(master)


class CliGate(unittest.TestCase):
    def test_legacy_connection_without_receipt_is_denied(self):
        code, output, confirmed = terminal(['connect', OWNER, 'fixture_client', FLAG])
        self.assertNotEqual(code, 0)
        self.assertFalse(confirmed)
        self.assertNotIn(b'https://x.com/', output)

    def test_terminal_confirmation_cannot_authorize_another_identity(self):
        code, output, confirmed = terminal(['connect', str(uuid.uuid4()), 'fixture_client', str(uuid.uuid4()), FLAG])
        self.assertNotEqual(code, 0)
        self.assertTrue(confirmed)
        self.assertNotIn(b'Open this URL', output)
        self.assertIn(b'STOPPED', output)

    @unittest.skipIf(Path('/usr/local/libexec/supa-x-owner-authorize').exists(),
                     'never exercise an installed real administrator authority from a synthetic test')
    def test_correct_owner_without_installed_authority_fails_before_consent(self):
        code, output, confirmed = terminal(['connect', OWNER, 'fixture_client', str(uuid.uuid4()), FLAG])
        self.assertNotEqual(code, 0)
        self.assertTrue(confirmed)
        self.assertNotIn(b'Open this URL', output)
        self.assertNotIn(b'https://x.com/', output)
        self.assertIn(b'STOPPED', output)


if __name__ == '__main__': unittest.main()
