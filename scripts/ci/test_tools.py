"""Tool downloads cannot execute unverified bytes or extract archive paths."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import tools


class ToolTests(unittest.TestCase):
    def install(self, member, expected_digest=None):
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode='w:gz') as bundle:
            entry = tarfile.TarInfo(member)
            entry.size = 4
            bundle.addfile(entry, io.BytesIO(b'tool'))
        data = archive.getvalue()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / '.github').mkdir()
            (root / '.github/infra-tools.json').write_text(json.dumps({'tool': {
                'version': '1', 'platforms': {'linux-amd64': {
                    'url': 'https://github.com/owner/tool/releases/download/v1/tool.tar.gz',
                    'sha256': expected_digest or hashlib.sha256(data).hexdigest()}}}}))
            with patch.object(tools, 'ROOT', root), patch('tools.platform.machine', return_value='x86_64'), \
                    patch('tools.platform.system', return_value='Linux'), \
                    patch('tools.urllib.request.urlopen', return_value=io.BytesIO(data)):
                result = Path(tools.ensure('tool'))
                self.assertEqual(result.read_bytes(), b'tool')
                self.assertTrue(result.stat().st_mode & 0o111)
                self.assertFalse((root / 'escaped').exists())

    def test_installs_one_verified_binary_without_extracting_archive_paths(self):
        self.install('../../escaped/tool')

    def test_rejects_checksum_mismatch(self):
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.install('tool', '0' * 64)

    def test_rejects_archive_without_expected_binary(self):
        with self.assertRaisesRegex(ValueError, 'expected one executable'):
            self.install('unexpected')
