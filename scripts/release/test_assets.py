"""Exercise the publication step against a simulated GitHub asset store."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from assets import REQUIRED


class PublicationTests(unittest.TestCase):
    def publish(self, omit_archive=False):
        source = Path(__file__).resolve().parents[2]
        workflow = (source / '.github/workflows/release.yml').read_text()
        step = re.split(r'      - name: Upload [^\n]+ and publish\n', workflow)[1].split('\n      - name:', 1)[0]
        script = textwrap.dedent(step.split('        run: |\n', 1)[1])
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(source / 'scripts', root / 'scripts', ignore=shutil.ignore_patterns('__pycache__'))
            shutil.copyfile(source / 'CHANGELOG.md', root / 'CHANGELOG.md')
            shutil.copyfile(source / 'scripts/release/assets.py', root / 'release-assets-check.py')
            assets = root / 'release-assets'
            assets.mkdir()
            for name in REQUIRED - {'release.json'}:
                (assets / name).write_text('verified fixture')
            gh = root / 'gh'
            gh.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
record = Path(os.environ['RELEASE_TEST_RECORD'])
state = json.loads(record.read_text()) if record.exists() else {'assets': [], 'published': False}
operation = sys.argv[2]
if operation == 'upload':
    for argument in sys.argv[4:]:
        if argument.startswith('--'):
            continue
        path = Path(argument)
        if os.environ.get('OMIT_ARCHIVE') and path.name.endswith('.tar.gz'):
            continue
        state['assets'].append({'name': path.name, 'size': path.stat().st_size})
elif operation == 'view':
    print(json.dumps({'assets': state['assets']}))
elif operation == 'edit':
    state['published'] = True
else:
    raise SystemExit('Unexpected gh command')
record.write_text(json.dumps(state))
''')
            gh.chmod(0o755)
            record = root / 'record.json'
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', RUNNER_TEMP=str(root),
                       RELEASE_TEST_RECORD=str(record), TAG='v0.0.2', SHA='a' * 40)
            if omit_archive:
                env['OMIT_ARCHIVE'] = '1'
            result = subprocess.run(['sh', '-eu', '-c', script], cwd=root, env=env, text=True, capture_output=True)
            return result, json.loads(record.read_text())

    def test_all_binaries_and_metadata_are_uploaded_before_publication(self):
        result, state = self.publish()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual({asset['name'] for asset in state['assets']}, REQUIRED)
        self.assertTrue(state['published'])

    def test_missing_remote_archives_leave_the_release_in_draft(self):
        result, state = self.publish(omit_archive=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Release assets missing or empty', result.stderr)
        self.assertFalse(state['published'])
