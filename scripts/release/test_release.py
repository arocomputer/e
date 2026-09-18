"""Release contracts that must agree across builds, channels, and the public changelog."""
import unittest
from identity import identity, version_key
from notes import parse


class ReleaseContracts(unittest.TestCase):
    def test_channel_versions_and_numeric_order(self):
        self.assertEqual(identity('v1.2.3')['npm_tag'], 'latest')
        self.assertEqual(identity('v1.2.3')['repository'], 'intuitums/e')
        self.assertEqual(identity('v0.0.0-beta-12')['repository'], 'intuitums/e-beta')
        self.assertEqual(identity('v0.0.0-dev-12')['repository'], '')
        self.assertEqual(identity('v0.0.0-beta-12')['command'], 'e-beta')
        self.assertGreater(version_key('0.0.0-dev-12'), version_key('0.0.0-dev-9'))
        for invalid in ['1.2.3-rc.1', '1.2.3-dev', '1.2.3-dev.1.g../file', '01.2.3']:
            with self.assertRaises(ValueError):
                identity(invalid)

    def test_display_titles_keep_versions_separate(self):
        for version, title in [
            ('1.2.3', '1.2.3'),
            ('0.0.0-beta-12', '0.0.0 · Beta 12'),
            ('0.0.0-dev-9', '0.0.0 · Dev 9'),
            ('0.0.0-pr-42', '0.0.0 · PR 42'),
        ]:
            with self.subTest(version=version):
                release = identity(version)
                self.assertEqual(release['title'], title)
                self.assertEqual(release['version'], version)

    def test_release_asset_preserves_grouped_markdown_and_continuations(self):
        result = parse('2026-09-15\n\n### Easier testing\n\nTry a preview.\n\n### New features\n- Run `e-dev`\n  alongside stable.\n### Improvements\n- **Upgrade:** Sign in separately.\n### Fixes\n- Keep updates in their channel.')
        self.assertEqual(result['title'], 'Easier testing')
        self.assertEqual(result['groups']['New features'], ['Run `e-dev` alongside stable.'])
        with self.assertRaises(ValueError):
            parse('### A title\n- Ungrouped change')

class BetaPromotion(unittest.TestCase):
    def test_only_beta_latest_advances_and_older_retries_do_not(self):
        import json
        from unittest.mock import patch
        from channel import advance
        for tag, newer, promotes in [
            ('v1.2.3', False, False),
            ('v0.0.0-dev-12', False, False),
            ('v0.0.0-beta-12', False, True),
            ('v0.0.0-beta-12', True, False),
        ]:
            pages = [[{'tag_name': 'v0.0.0-beta-13', 'draft': False, 'prerelease': False}]] if newer else [[]]
            with patch('channel.subprocess.check_output', return_value=json.dumps(pages)), \
                    patch('channel.subprocess.run') as run:
                advance(tag)
            if promotes:
                run.assert_called_once_with(['gh', 'release', 'edit', tag, '--repo', 'intuitums/e-beta', '--latest'], check=True)
            else:
                run.assert_not_called()


class ShellInstaller(unittest.TestCase):
    def test_public_dev_install_redirects_to_npm_without_downloading(self):
        import os
        from pathlib import Path
        import subprocess
        root = Path(__file__).resolve().parents[2]
        env = {key: value for key, value in os.environ.items() if key != 'E_RELEASE_BASE'}
        result = subprocess.run(['sh', str(root / 'install.sh'), '--channel', 'dev'], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn('npm install -g @intuitums/e@dev', result.stderr)

    def test_pinned_beta_keeps_production_and_rejects_a_mismatched_channel(self):
        import hashlib
        import io
        import os
        from pathlib import Path
        import platform
        import subprocess
        import tarfile
        import tempfile
        root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            output = work / 'bin'
            output.mkdir()
            (output / 'e').write_text('stable remains here')
            arch = 'aarch64' if platform.machine() in ('arm64','aarch64') else 'x86_64'
            target = arch + ('-apple-darwin' if platform.system() == 'Darwin' else '-unknown-linux-gnu')
            archive = work / f'e-{target}.tar.gz'
            version = '0.0.0-beta-12'
            binary = f'#!/bin/sh\necho "e {version}"\n'.encode()
            with tarfile.open(archive, 'w:gz') as tar:
                member = tarfile.TarInfo('e')
                member.size, member.mode = len(binary), 0o755
                tar.addfile(member, io.BytesIO(binary))
            (work / 'checksums.txt').write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
            env = dict(os.environ, E_RELEASE_BASE=work.as_uri(), E_INSTALL_DIR=str(output))
            result = subprocess.run(['sh',str(root/'install.sh'),'--channel','beta','--version',version],env=env,capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual((output/'e').read_text(),'stable remains here')
            self.assertTrue((output/'e-beta').exists())
            bad = subprocess.run(['sh',str(root/'install.sh'),'--version',version],env=env,capture_output=True)
            self.assertNotEqual(bad.returncode,0)
