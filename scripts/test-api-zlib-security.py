#!/usr/bin/env python3
"""Offline API runtime contract; optionally verify the actual vendor package ABI.

The optional package must already have signed-repository provenance verified.
This script neither downloads packages nor installs or starts services.
"""
import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
VERSION = '1:1.3.dfsg+really1.3.1-1+deb13u1'
PACKAGE_SHA256 = '8c27c342230503b73dfde629642c3b352e87b554e06d60cf58772f236a305ceb'
BASE = 'debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2'


def checked(args, **kwargs):
    result = subprocess.run(args, capture_output=True, timeout=30, **kwargs)
    if result.returncode:
        raise ValueError('package_verification_command_failed')
    return result.stdout.decode()


def verify_package(package, original_library, include_dir=None):
    if package.is_symlink() or not package.is_file():
        raise ValueError('regular_vendor_package_required')
    if hashlib.sha256(package.read_bytes()).hexdigest() != PACKAGE_SHA256:
        raise ValueError('vendor_package_checksum_mismatch')
    for field, value in [('Package', 'zlib1g'), ('Version', VERSION), ('Architecture', 'amd64')]:
        if checked(['dpkg-deb', '--field', str(package), field]).strip() != value:
            raise ValueError('vendor_package_identity_mismatch')
    with tempfile.TemporaryDirectory(prefix='supa-api-zlib-') as temp:
        root = Path(temp)
        checked(['dpkg-deb', '--extract', str(package), str(root / 'overlay')])
        checked(['dpkg-deb', '--control', str(package), str(root / 'control')])
        library = root / 'overlay/usr/lib/x86_64-linux-gnu/libz.so.1.3.1'
        if (library.parent / 'libz.so.1').resolve() != library.resolve():
            raise ValueError('vendor_soname_target_mismatch')
        dynamic = checked(['readelf', '-d', str(library)])
        if re.findall(r'Shared library: \[(.*?)\]', dynamic) != ['libc.so.6']:
            raise ValueError('unexpected_runtime_dependencies')
        def symbols(path):
            rows = checked(['readelf', '--dyn-syms', '--wide', str(path)]).splitlines()
            return {r.split()[-1] for r in rows if len(r.split()) >= 8
                    and r.split()[6] != 'UND' and r.split()[4] in ('GLOBAL', 'WEAK')}
        if not symbols(original_library) <= symbols(library):
            raise ValueError('original_abi_exports_removed')
        probe = root / 'probe'
        checked(['gcc', '-Wall', '-Wextra', '-Werror',
                 *(['-I' + str(include_dir)] if include_dir else []),
                 str(ROOT / 'scripts/fixtures/zlib-gzwrite-recovery.c'), str(library),
                 '-Wl,-rpath,' + str(library.parent), '-o', str(probe)])
        if str(library.parent / 'libz.so.1') not in checked(['ldd', str(probe)]):
            raise ValueError('patched_library_not_loaded')
        if 'gzwrite recovery and normal gzip roundtrip passed' not in checked([str(probe)], cwd=root):
            raise ValueError('gzip_recovery_failed')


class RuntimeContract(unittest.TestCase):
    def setUp(self):
        self.source = (ROOT / 'deployment/api.Dockerfile').read_text()
        self.overlay = self.source.split(' AS zlib-security\n', 1)[1].split('\nFROM ', 1)[0]
        self.runtime = self.source.split(' AS runtime\n', 1)[1]

    def test_signed_vendor_download_is_exact_and_checksum_checked(self):
        self.assertIn('FROM ' + BASE + ' AS zlib-security', self.source)
        self.assertIn('apt-get update', self.overlay)
        self.assertIn('apt-get download zlib1g=' + VERSION, self.overlay)
        self.assertIn(PACKAGE_SHA256, self.overlay)
        self.assertIn('sha256sum --check --strict', self.overlay)
        self.assertNotRegex(self.source, r'allow-unauthenticated|allow-insecure|trusted=yes')

    def test_wrong_architecture_rejected(self):
        self.assertIn('test "$(dpkg --print-architecture)" = amd64', self.overlay)

    def test_vendor_payload_and_scanner_inventory_are_both_preserved(self):
        self.assertIn('dpkg-deb --extract', self.overlay)
        self.assertIn('dpkg-deb --control', self.overlay)
        self.assertIn('cp /control/control /overlay/var/lib/dpkg/status.d/zlib1g', self.overlay)
        self.assertIn('cp /control/md5sums /overlay/var/lib/dpkg/status.d/zlib1g.md5sums', self.overlay)
        self.assertIn('COPY --from=zlib-security /overlay/ /', self.runtime)

    def test_runtime_remains_minimal_nonroot_and_offline(self):
        self.assertIn('USER 65532:65532', self.runtime)
        self.assertIn('SUPA_API_SCHEDULER_ENABLED=false SUPA_WALLET_ENABLED=false', self.runtime)
        self.assertIn('SUPA_PROVIDER=offline', self.runtime)
        self.assertNotIn('RUN ', self.runtime)
        self.assertIn('ENTRYPOINT ["/usr/local/bin/supa-api"]', self.runtime)

    def test_changed_package_rejected_before_extraction(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'changed.deb'
            path.write_bytes(b'synthetic altered package')
            with self.assertRaisesRegex(ValueError, 'vendor_package_checksum_mismatch'):
                verify_package(path, Path('/nonexistent'))

    def test_symlink_package_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'link.deb'
            path.symlink_to('/nonexistent')
            with self.assertRaisesRegex(ValueError, 'regular_vendor_package_required'):
                verify_package(path, Path('/nonexistent'))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path)
    parser.add_argument('--original-library', type=Path)
    parser.add_argument('--include-dir', type=Path, help='Extracted matching vendor zlib headers; never installed')
    args = parser.parse_args()
    if bool(args.package) != bool(args.original_library):
        parser.error('package and original-library must be supplied together')
    if args.package:
        verify_package(args.package.absolute(), args.original_library.resolve(strict=True), args.include_dir)
        print('Actual vendor package: identity, ABI, loading and gzip recovery passed.')
    unittest.main(argv=[__file__])
