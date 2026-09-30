"""Reject unreviewed/source-mismatched publication; preserve independent SDK versions."""
import io
import json
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest
from registry import verify, payload


class RegistryTests(unittest.TestCase):
    def fixture(self, root):
        def git(*args):
            return subprocess.check_output(['git', *args], cwd=root, text=True).strip()
        git('init', '-q')
        git('config', 'user.name', 'Fixture')
        git('config', 'user.email', 'fixture@example.invalid')
        (root / 'crates/sdk').mkdir(parents=True)
        (root / 'Cargo.toml').write_text('[workspace.package]\nversion="1.2.3"\n')
        (root / 'crates/sdk/Cargo.toml').write_text('[package]\nversion="4.5.6"\n[dependencies.e-core]\nversion="=1.2.3"\n')
        git('add', '.')
        git('commit', '-qm', 'release fixture')
        git('tag', 'v1.2.3')
        git('update-ref', 'refs/remotes/origin/main', 'HEAD')
        return git

    def test_selected_source_and_independent_versions_are_preserved(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            git = self.fixture(root)
            commit, packages = verify(root, 'v1.2.3')
            self.assertEqual(commit, git('rev-parse', 'HEAD'))
            self.assertEqual(packages, [('e-core', '1.2.3'), ('e-sdk', '4.5.6')])
            (root / 'new.txt').write_text('later dispatch source')
            git('add', '.')
            git('commit', '-qm', 'later main')
            git('update-ref', 'refs/remotes/origin/main', 'HEAD')
            with self.assertRaisesRegex(ValueError, 'selected release tag'):
                verify(root, 'v1.2.3')

    def test_dirty_or_off_main_source_cannot_publish(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            git = self.fixture(root)
            (root / 'Cargo.toml').write_text('[workspace.package]\nversion="9.9.9"\n')
            with self.assertRaises(subprocess.CalledProcessError):
                verify(root, 'v1.2.3')
            git('checkout', '--', 'Cargo.toml')
            git('checkout', '--orphan', 'other')
            git('commit', '-qm', 'unrelated history')
            git('update-ref', 'refs/remotes/origin/main', 'HEAD')
            git('checkout', '--detach', 'v1.2.3')
            with self.assertRaises(subprocess.CalledProcessError):
                verify(root, 'v1.2.3')

    def test_upload_preserves_exact_archive_and_renamed_dependency(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / 'e-sdk-4.5.6.crate'
            manifest = b'[package]\nname="e-sdk"\nversion="4.5.6"\n[dependencies.alias]\npackage="e-core"\nversion="=1.2.3"\n'
            with tarfile.open(archive, 'w:gz') as bundle:
                member = tarfile.TarInfo('e-sdk-4.5.6/Cargo.toml')
                member.size = len(manifest)
                bundle.addfile(member, io.BytesIO(manifest))
            upload = payload(archive)
            size = struct.unpack('<I', upload[:4])[0]
            metadata = json.loads(upload[4:4+size])
            self.assertEqual(metadata['deps'][0]['name'], 'e-core')
            self.assertEqual(metadata['deps'][0]['explicit_name_in_toml'], 'alias')
            self.assertEqual(upload[8+size:], archive.read_bytes())


if __name__ == '__main__':
    unittest.main()
