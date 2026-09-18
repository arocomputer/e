"""A successful unit run alone must never authorize a dev release."""
import json
import subprocess
import unittest
from unittest.mock import patch
from checks import WORKFLOWS, select, verify

SHA = 'a' * 40


def runs():
    """Represent independent successful checks for one main commit."""
    return [{'id': i + 1, 'path': f'.github/workflows/{name}.yml',
             'head_sha': SHA, 'head_branch': 'main', 'event': 'push',
             'status': 'completed', 'conclusion': 'success'}
            for i, name in enumerate(WORKFLOWS)]


class SourceChecks(unittest.TestCase):
    def test_requires_every_workflow_from_the_same_main_push(self):
        for replacement in [None, {'head_sha': 'b' * 40}, {'event': 'pull_request'},
                            {'head_branch': 'topic'}]:
            evidence = runs()
            removed = evidence.pop()
            if replacement:
                evidence.append(removed | replacement)
            with self.subTest(replacement=replacement), self.assertRaisesRegex(ValueError, 'Missing'):
                select([{'workflow_runs': evidence}], SHA)

    def test_latest_run_wins_across_pages(self):
        evidence = runs()
        retry = evidence[0] | {'id': 100, 'conclusion': 'failure'}
        selected = select([{'workflow_runs': [retry]}, {'workflow_runs': evidence}], SHA)
        self.assertEqual(selected[0], retry)

    def test_failures_and_cancellations_prevent_publication(self):
        for conclusion in ['failure', 'cancelled', 'timed_out', 'skipped']:
            evidence = runs()
            evidence[-1]['conclusion'] = conclusion
            with self.subTest(conclusion=conclusion), \
                    patch.dict('os.environ', GITHUB_REPOSITORY='intuitums/e'), \
                    patch('checks.subprocess.check_output', return_value=json.dumps([{'workflow_runs': evidence}])), \
                    self.assertRaisesRegex(ValueError, 'Source check failed'):
                verify(SHA)

    def test_waits_for_running_work_and_propagates_its_failure(self):
        evidence = runs()
        evidence[-1].update(status='in_progress', conclusion=None)
        with patch.dict('os.environ', GITHUB_REPOSITORY='intuitums/e'), \
                patch('checks.subprocess.check_output', return_value=json.dumps([{'workflow_runs': evidence}])), \
                patch('checks.subprocess.run', side_effect=subprocess.CalledProcessError(1, 'gh')) as watch, \
                self.assertRaises(subprocess.CalledProcessError):
            verify(SHA)
        self.assertIn('--exit-status', watch.call_args.args[0])
