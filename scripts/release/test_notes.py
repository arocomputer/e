"""Release bodies remain the source for both GitHub and the website."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
BODY = '### Better editing\n\nKeep your place.\n\n### Fixes\n- Restore the cursor.\n'


class DraftNotes(unittest.TestCase):
    def read(self, release, *, status=0, tag='v1.2.3'):
        """Run the real notes reader with a local GitHub CLI fixture."""
        with tempfile.TemporaryDirectory() as tmp:
            cli = Path(tmp, 'gh')
            cli.write_text('#!/usr/bin/env python3\nimport sys\n'
                           + f'assert sys.argv[1:] == {["release", "view", tag, "--repo", "intuitums/e", "--json", "isDraft,body"]!r}\n'
                           + f'print({json.dumps(release)!r})\nsys.exit({status})\n')
            cli.chmod(0o755)
            return subprocess.run(
                ['sh', str(ROOT / 'scripts/release-notes.sh'), tag],
                env=dict(os.environ, PATH=tmp + os.pathsep + os.environ['PATH']),
                capture_output=True, text=True)

    def test_preserves_reviewed_body_verbatim(self):
        result = self.read({'isDraft': True, 'body': BODY})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, BODY)

    def test_missing_published_and_malformed_drafts_fail_without_notes(self):
        for release, status in [
            ({}, 1),
            ({'isDraft': True, 'body': BODY}, 1),
            ({'isDraft': False, 'body': BODY}, 0),
            ({'isDraft': True, 'body': '- No release title or group'}, 0),
            ({'isDraft': True, 'body': ''}, 0),
        ]:
            with self.subTest(release=release):
                result = self.read(release, status=status)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, '')

    def test_rejects_nonstable_tags(self):
        for tag in ['1.2.3', 'v1.2.3-beta.1', 'v01.2.3', 'v1x2x3']:
            with self.subTest(tag=tag):
                result = self.read({'isDraft': True, 'body': BODY}, tag=tag)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, '')
