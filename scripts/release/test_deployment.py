"""Deployment history must reflect verified installers and the actual release source."""
import json
import os
import unittest
from unittest.mock import patch
from deployment import report


class DeploymentTests(unittest.TestCase):
    def test_production_maps_to_the_production_environment(self):
        for outcome in ['success', 'failure', 'cancelled']:
            with self.subTest(outcome=outcome):
                needs = {
                    'resolve': {'result': 'success', 'outputs': {
                        'channel': 'production', 'repository': 'arocomputer/ulo', 'sha': 'a' * 40, 'tag': 'v1.2.3', 'version': '1.2.3'}},
                    'npm': {'result': outcome}, 'homebrew': {'result': 'success'},
                    'channel': {'result': 'success' if outcome == 'success' else 'skipped'},
                }
                with patch.dict(os.environ, GITHUB_STEP_SUMMARY=''), \
                        patch('deployment.subprocess.check_output', side_effect=['{"id": 42}', '{}']) as call:
                    report(needs, 'arocomputer/ulo', 'https://github.com', '123')
                created, status = [json.loads(c.kwargs['input']) for c in call.call_args_list]
                self.assertEqual(created['environment'], 'production')
                self.assertEqual(created['ref'], 'a' * 40)
                self.assertFalse(created['auto_merge'])
                self.assertTrue(created['production_environment'])
                self.assertEqual(status['state'], 'error' if outcome == 'cancelled' else outcome)
                self.assertEqual(status['log_url'], 'https://github.com/arocomputer/ulo/actions/runs/123')
                self.assertEqual(status['environment_url'],
                                 'https://github.com/arocomputer/ulo/releases/tag/v1.2.3' if outcome == 'success' else status['log_url'])

    def test_failed_npm_does_not_hide_verified_direct_downloads(self):
        from pathlib import Path
        import tempfile
        needs = {'resolve': {'result': 'success', 'outputs': {'channel': 'production', 'sha': 'a' * 40,
                 'repository': 'arocomputer/ulo', 'tag': 'v1.2.3', 'version': '1.2.3'}},
                 'publish': {'result': 'success'}, 'channel': {'result': 'success'},
                 'npm': {'result': 'failure'}, 'homebrew': {'result': 'success'}}
        with tempfile.TemporaryDirectory() as tmp, \
                patch.dict(os.environ, GITHUB_STEP_SUMMARY=str(Path(tmp, 'summary'))), \
                patch('deployment.subprocess.check_output', side_effect=['{"id": 42}', '{}']) as call:
            report(needs, 'arocomputer/ulo', 'https://github.com', '123')
            summary = Path(tmp, 'summary').read_text()
        self.assertIn('| Website installer | success |', summary)
        self.assertIn('| npm and bun | failure |', summary)
        self.assertEqual(json.loads(call.call_args.kwargs['input'])['state'], 'failure')


if __name__ == '__main__':
    unittest.main()
