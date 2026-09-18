"""Only verified official dev artifacts can reach the existing checksum installer."""
import json
from pathlib import Path
import unittest
from unittest.mock import patch
from dev_install import install


class DevInstallTests(unittest.TestCase):
    def fixture(self, *, verified=True, event='workflow_run', channel='dev', job='release'):
        """Supply downloaded metadata without contacting GitHub or changing an installation."""
        def gh(*args):
            if args[0] == 'run':
                Path(args[-1], 'build.json').write_text(json.dumps({
                    'version': f'0.0.0-{channel}-12', 'commit': 'abcdef012345' + 'a' * 28}))
                return ''
            if 'jobs?' in args[-1]:
                return json.dumps([{'jobs': [{'name': job,
                                             'conclusion': 'success' if verified else 'failure'}]}])
            return json.dumps({'path': '.github/workflows/release.yml', 'event': event,
                               'head_branch': 'main', 'head_sha': 'abcdef012345' + 'a' * 28,
                               'conclusion': 'failure'})
        return gh

    def test_installs_verified_assets_even_when_npm_failed(self):
        with patch('dev_install.gh', side_effect=self.fixture()), patch('dev_install.subprocess.run') as run:
            install('123')
        self.assertEqual(run.call_args.args[0][-4:], ['--channel', 'dev', '--version', '0.0.0-dev-12'])
        self.assertTrue(run.call_args.kwargs['env']['E_RELEASE_BASE'].startswith('file://'))

    def test_previous_workflow_job_name_remains_installable(self):
        with patch('dev_install.gh', side_effect=self.fixture(job='Checksums and publish')), \
                patch('dev_install.subprocess.run') as run:
            install('123')
        run.assert_called_once()

    def test_unverified_or_non_dev_runs_never_install(self):
        for options in [{'verified': False}, {'event': 'pull_request'}, {'channel': 'beta'}]:
            with self.subTest(options=options), patch('dev_install.gh', side_effect=self.fixture(**options)), \
                    patch('dev_install.subprocess.run') as run:
                with self.assertRaises(ValueError):
                    install('123')
                run.assert_not_called()

    def test_direct_install_checks_archive_and_preserves_production(self):
        import hashlib
        import io
        import os
        import platform
        import subprocess
        import tarfile
        import tempfile
        gh_fixture = self.fixture()
        for corrupt in [False, True]:
            def gh(*args):
                result = gh_fixture(*args)
                if args[0] == 'run':
                    root = Path(args[-1])
                    arch = 'aarch64' if platform.machine() in ('arm64', 'aarch64') else 'x86_64'
                    target = arch + ('-apple-darwin' if platform.system() == 'Darwin' else '-unknown-linux-gnu')
                    archive = root / f'e-{target}.tar.gz'
                    binary = b'#!/bin/sh\necho e 0.0.0-dev-12\n'
                    with tarfile.open(archive, 'w:gz') as tar:
                        member = tarfile.TarInfo('e')
                        member.size, member.mode = len(binary), 0o755
                        tar.addfile(member, io.BytesIO(binary))
                    digest = '0' * 64 if corrupt else hashlib.sha256(archive.read_bytes()).hexdigest()
                    (root / 'checksums.txt').write_text(f'{digest}  {archive.name}\n')
                return result
            with self.subTest(corrupt=corrupt), tempfile.TemporaryDirectory() as tmp, \
                    patch.dict(os.environ, E_INSTALL_DIR=tmp), patch('dev_install.gh', side_effect=gh):
                Path(tmp, 'e').write_text('stable remains here')
                if corrupt:
                    with self.assertRaises(subprocess.CalledProcessError):
                        install('123')
                    self.assertFalse(Path(tmp, 'e-dev').exists())
                else:
                    install('123')
                    self.assertTrue(Path(tmp, 'e-dev').exists())
                self.assertEqual(Path(tmp, 'e').read_text(), 'stable remains here')
