"""Deployment history must reflect verified installers and the actual release source."""
import json
import os
import unittest
from unittest.mock import patch
from deployment import report


class DeploymentTests(unittest.TestCase):
    def test_channel_mapping_and_failed_publication(self):
        for channel, environment in [('stable', 'production'), ('beta', 'beta'), ('dev', 'dev')]:
            for outcome in ['success', 'failure', 'cancelled']:
                with self.subTest(channel=channel, outcome=outcome):
                    needs = {
                        'resolve': {'result': 'success', 'outputs': {
                            'channel': channel, 'repository': 'intuitums/e-beta' if channel == 'beta' else 'intuitums/e', 'sha': 'a' * 40, 'tag': 'v1.2.3', 'version': '1.2.3'}},
                        'npm': {'result': outcome}, 'homebrew': {'result': 'skipped' if channel == 'dev' else 'success'},
                        'channel': {'result': 'success' if outcome == 'success' else 'skipped'},
                    }
                    with patch.dict(os.environ, GITHUB_STEP_SUMMARY=''), \
                            patch('deployment.subprocess.check_output', side_effect=['{"id": 42}', '{}']) as call:
                        report(needs, 'intuitums/e', 'https://github.com', '123')
                    created, status = [json.loads(c.kwargs['input']) for c in call.call_args_list]
                    self.assertEqual(created['environment'], environment)
                    self.assertEqual(created['ref'], 'a' * 40)
                    self.assertFalse(created['auto_merge'])
                    self.assertEqual(created['production_environment'], channel == 'stable')
                    self.assertEqual(status['state'], 'error' if outcome == 'cancelled' else outcome)
                    self.assertEqual(status['log_url'], 'https://github.com/intuitums/e/actions/runs/123')
                    self.assertEqual(status['environment_url'],
                                     (('https://www.npmjs.com/package/@intuitums/e/v/1.2.3' if channel == 'dev' else f'https://github.com/intuitums/{"e-beta" if channel == "beta" else "e"}/releases/tag/v1.2.3') if outcome == 'success' else status['log_url']))

    def test_failed_npm_does_not_hide_verified_direct_downloads(self):
        from pathlib import Path
        import tempfile
        needs = {'resolve': {'result': 'success', 'outputs': {'channel': 'beta', 'sha': 'a' * 40,
                 'repository': 'intuitums/e-beta', 'tag': 'v1.2.3-beta.1.gaaaaaaaaaaaa',
                 'version': '1.2.3-beta.1.gaaaaaaaaaaaa'}},
                 'publish': {'result': 'success'}, 'channel': {'result': 'success'},
                 'npm': {'result': 'failure'}, 'homebrew': {'result': 'success'}}
        with tempfile.TemporaryDirectory() as tmp, \
                patch.dict(os.environ, GITHUB_STEP_SUMMARY=str(Path(tmp, 'summary'))), \
                patch('deployment.subprocess.check_output', side_effect=['{"id": 42}', '{}']) as call:
            report(needs, 'intuitums/e', 'https://github.com', '123')
            summary = Path(tmp, 'summary').read_text()
        self.assertIn('| Website installer | success |', summary)
        self.assertIn('| npm and bun | failure |', summary)
        self.assertEqual(json.loads(call.call_args.kwargs['input'])['state'], 'failure')
