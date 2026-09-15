"""Deployment history must reflect verified installers and the actual release source."""
import json
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
                            'channel': channel, 'sha': 'a' * 40, 'tag': 'v1.2.3', 'version': '1.2.3'}},
                        'npm': {'result': outcome}, 'homebrew': {'result': 'success'},
                        'channel': {'result': 'success' if outcome == 'success' else 'skipped'},
                    }
                    with patch('deployment.subprocess.check_output', side_effect=['{"id": 42}', '{}']) as call:
                        report(needs, 'intuitums/e', 'https://github.com', '123')
                    created, status = [json.loads(c.kwargs['input']) for c in call.call_args_list]
                    self.assertEqual(created['environment'], environment)
                    self.assertEqual(created['ref'], 'a' * 40)
                    self.assertFalse(created['auto_merge'])
                    self.assertEqual(created['production_environment'], channel == 'stable')
                    self.assertEqual(status['state'], 'error' if outcome == 'cancelled' else outcome)
                    self.assertEqual(status['log_url'], 'https://github.com/intuitums/e/actions/runs/123')
                    self.assertEqual(status['environment_url'],
                                     'https://github.com/intuitums/e/releases/tag/v1.2.3' if outcome == 'success' else status['log_url'])
