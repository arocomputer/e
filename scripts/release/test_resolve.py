"""Keep draft recovery tied to its recorded source before GitHub creates the tag."""
import json
import os
import unittest
from unittest.mock import patch

from resolve import resolve


class RetryTests(unittest.TestCase):
    def test_draft_recovers_without_a_tag(self):
        tag = 'v0.0.0-beta-9'
        sha = 'd2b45315c43c8f78ea78c7d17043040e2b1dbf7d'
        event = {'inputs': {'action': 'retry', 'tag': tag}}
        with patch.dict(os.environ, GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_EVENT_PATH='/event'), \
                patch('resolve.Path.read_text', return_value=json.dumps(event)), \
                patch('resolve.subprocess.check_output', return_value=json.dumps({'isDraft': True, 'targetCommitish': 'beta-repository-main', 'body': f'Source: https://github.com/intuitums/e/commit/{sha}'})), \
                patch('resolve.git', return_value=sha) as git, \
                patch('resolve.subprocess.run') as run:
            result = resolve()
        git.assert_called_once_with('rev-parse', '--verify', f'{sha}^{{commit}}')
        run.assert_called_once_with(['git', 'merge-base', '--is-ancestor', sha, 'origin/main'], check=True)
        self.assertEqual(result['mode'], 'recover')
        self.assertEqual(result['sha'], sha)

    def test_production_selection_requires_beta_verification(self):
        with patch.dict(os.environ, GITHUB_EVENT_NAME='push', GITHUB_EVENT_PATH='/event',
                        GITHUB_REF_NAME='v1.2.3'), \
                patch('resolve.Path.read_text', return_value='{}'), \
                patch('resolve.git', side_effect=['a' * 40, '[package]\nversion = "1.2.3"']), \
                patch('resolve.subprocess.run'), \
                patch('resolve.verify', side_effect=ValueError('beta required')) as verify:
            with self.assertRaisesRegex(ValueError, 'beta required'):
                resolve()
        verify.assert_called_once_with('v1.2.3', 'a' * 40)

    def test_dev_publication_uses_the_shared_path_filter(self):
        sha = 'a' * 40
        event = {'workflow_run': {'conclusion': 'success', 'head_branch': 'main',
                 'event': 'push', 'head_sha': sha,
                 'head_repository': {'full_name': 'intuitums/e'}}}
        for paths, mode in [('README.md\nassets/readme.png', 'skip'),
                            ('crates/core/themes/dark.json', 'build')]:
            with self.subTest(paths=paths), patch.dict(os.environ,
                    GITHUB_EVENT_NAME='workflow_run', GITHUB_EVENT_PATH='/event',
                    GITHUB_REPOSITORY='intuitums/e', GITHUB_RUN_NUMBER='12'), \
                    patch('resolve.Path.read_text', return_value=json.dumps(event)), \
                    patch('resolve.git', side_effect=['[workspace.package]\nversion = "1.2.3"', paths]), \
                    patch('resolve.subprocess.run'):
                self.assertEqual(resolve()['mode'], mode)

    def test_dev_retry_requires_the_original_run(self):
        event = {'inputs': {'action': 'retry', 'tag': 'v0.0.0-dev-12'}}
        with patch.dict(os.environ, GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_EVENT_PATH='/event'), \
                patch('resolve.Path.read_text', return_value=json.dumps(event)), \
                patch('resolve.subprocess.check_output') as gh:
            with self.assertRaisesRegex(AssertionError, 'original Actions run'):
                resolve()
        gh.assert_not_called()


if __name__ == '__main__':
    unittest.main()
