"""Keep draft recovery tied to its recorded source before GitHub creates the tag."""
import json
import os
import unittest
from unittest.mock import patch

from resolve import resolve


class RetryTests(unittest.TestCase):
    def test_draft_recovers_without_a_tag(self):
        tag = 'v0.0.1-beta.9.gd2b45315c43c'
        sha = 'd2b45315c43c8f78ea78c7d17043040e2b1dbf7d'
        event = {'inputs': {'action': 'retry', 'tag': tag}}
        with patch.dict(os.environ, GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_EVENT_PATH='/event'), \
                patch('resolve.Path.read_text', return_value=json.dumps(event)), \
                patch('resolve.subprocess.check_output', return_value=json.dumps({'isDraft': True, 'targetCommitish': sha})), \
                patch('resolve.git', return_value=sha) as git, \
                patch('resolve.subprocess.run') as run:
            result = resolve()
        git.assert_called_once_with('rev-parse', '--verify', f'{sha}^{{commit}}')
        run.assert_called_once_with(['git', 'merge-base', '--is-ancestor', sha, 'origin/main'], check=True)
        self.assertEqual(result['mode'], 'recover')
        self.assertEqual(result['sha'], sha)


if __name__ == '__main__':
    unittest.main()
