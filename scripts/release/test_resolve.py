"""Keep draft recovery tied to its recorded source before GitHub creates the tag."""
import json
import os
import unittest
from unittest.mock import patch

from resolve import resolve


class RetryTests(unittest.TestCase):
    def test_draft_recovers_without_a_tag(self):
        tag = 'v1.2.3'
        sha = 'd2b45315c43c8f78ea78c7d17043040e2b1dbf7d'
        event = {'inputs': {'action': 'retry', 'tag': tag}}
        with patch.dict(os.environ, GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_EVENT_PATH='/event'), \
                patch('resolve.Path.read_text', return_value=json.dumps(event)), \
                patch('resolve.subprocess.check_output', return_value=json.dumps({'isDraft': True, 'targetCommitish': 'main'})), \
                patch('resolve.git', return_value=sha) as git, \
                patch('resolve.subprocess.run') as run:
            result = resolve()
        git.assert_called_once_with('rev-parse', '--verify', 'main^{commit}')
        run.assert_called_once_with(['git', 'merge-base', '--is-ancestor', sha, 'origin/main'], check=True)
        self.assertEqual(result['mode'], 'recover')
        self.assertEqual(result['sha'], sha)

    def test_production_selection_matches_manifest_and_refuses_other_channels(self):
        with patch.dict(os.environ, GITHUB_EVENT_NAME='push', GITHUB_EVENT_PATH='/event',
                        GITHUB_REF_NAME='v1.2.3'), \
                patch('resolve.Path.read_text', return_value='{}'), \
                patch('resolve.git', side_effect=['a' * 40, '[workspace.package]\nversion = "1.2.3"']), \
                patch('resolve.subprocess.run'):
            result = resolve()
        self.assertEqual(result['channel'], 'production')
        self.assertEqual(result['tag'], 'v1.2.3')
        self.assertEqual(result['mode'], 'build')

    def test_retry_refuses_a_non_production_tag(self):
        event = {'inputs': {'action': 'retry', 'tag': 'v0.0.0-pr-12'}}
        with patch.dict(os.environ, GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_EVENT_PATH='/event'), \
                patch('resolve.Path.read_text', return_value=json.dumps(event)), \
                patch('resolve.subprocess.check_output') as gh:
            with self.assertRaisesRegex(AssertionError, 'Only production packages can be retried'):
                resolve()
        gh.assert_not_called()


if __name__ == '__main__':
    unittest.main()
