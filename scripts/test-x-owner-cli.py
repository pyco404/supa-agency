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


def terminal(args, confirmation=b'CONNECT @supa_agency ONCE\n'):
    master, slave = pty.openpty()
    def no_core(): resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    child = subprocess.Popen([str(BINARY), *args], stdin=slave, stdout=slave, stderr=slave,
        cwd=ROOT, env={'PATH': '/usr/bin', 'LANG': 'C.UTF-8', 'DBUS_SESSION_BUS_ADDRESS':'unix:path=/tmp/supa-nonexistent-test-bus'}, preexec_fn=no_core)
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
                if (b'Type CONNECT' in output or b'Type VERIFY' in output) and not confirmed:
                    os.write(master, confirmation)
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

    def test_identity_and_certificate_arguments_are_not_accepted_by_new_connector(self):
        code, output, confirmed = terminal(['connect', str(uuid.uuid4()), 'fixture_client', str(uuid.uuid4()), FLAG])
        self.assertNotEqual(code, 0)
        self.assertFalse(confirmed)
        self.assertNotIn(b'Open this URL', output)

    def test_unavailable_local_keyring_denies_before_any_consent_or_secret_read(self):
        code, output, confirmed = terminal(['connect-local','fixture_client',FLAG])
        self.assertNotEqual(code, 0)
        self.assertTrue(confirmed)
        self.assertNotIn(b'Open this URL', output)
        self.assertNotIn(b'https://x.com/', output)

    def test_cancelled_confirmation_never_starts_consent(self):
        code, output, confirmed = terminal(['connect-local','fixture_client',FLAG],b'CANCEL\n')
        self.assertNotEqual(code, 0)
        self.assertTrue(confirmed)
        self.assertNotIn(b'Open this URL', output)

    def test_oauth1_verification_needs_explicit_budget_flag_and_confirmation(self):
        code,output,confirmed=terminal(['verify-oauth1'])
        self.assertNotEqual(code,0)
        self.assertFalse(confirmed)
        code,output,confirmed=terminal(['verify-oauth1','--authorize-one-budgeted-identity-read'],b'CANCEL\n')
        self.assertNotEqual(code,0)
        self.assertTrue(confirmed)
        self.assertNotIn(b'account_id',output)

    def test_oauth1_setup_has_no_noninteractive_secret_input_fallback(self):
        result=subprocess.run([str(BINARY),'setup-oauth1'],input=b'fixture-secret-not-for-store',capture_output=True,timeout=3)
        self.assertNotEqual(result.returncode,0)
        self.assertNotIn(b'fixture-secret-not-for-store',result.stdout+result.stderr)


if __name__ == '__main__': unittest.main()
