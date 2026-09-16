"""Dev comments must identify the right PR, failure stage, and verified build."""
import unittest
from unittest.mock import patch
from pr_comment import MARKER, render, report


def jobs(result='success'):
    """A selected dev build with its publication outcome."""
    return {
        'resolve': {'outputs': {'channel': 'dev', 'mode': 'build', 'sha': 'a' * 40, 'version': '1.2.3-dev.1.gaaaaaaaaaaaa'}},
        'build': {'result': 'success'}, 'npm': {'result': result},
    }


class CommentTests(unittest.TestCase):
    def test_install_command_only_after_success(self):
        for stage in ('building', 'publishing', 'finished'):
            for result in ('success', 'failure', 'cancelled', 'skipped'):
                body = render(jobs(result), stage, 'https://github.com/intuitums/e/actions/runs/1')
                self.assertEqual('npm install -g' in body, stage == 'finished' and result == 'success')
        self.assertIn('@intuitums/e@1.2.3-dev.1.gaaaaaaaaaaaa', render(jobs(), 'finished', 'url'))

    def test_install_failure_is_not_called_a_build_failure(self):
        needs = jobs('failure')
        needs['npm']['outputs'] = {'verify': 'failure'}
        body = render(needs, 'finished', 'url')
        self.assertIn('Installation verification failed', body)
        self.assertNotIn('Native build failure', body)

    def test_updates_bot_comment_only_on_matching_merged_pr(self):
        matching = {'number': 1, 'merged_at': 'today', 'merge_commit_sha': 'a' * 40,
                    'base': {'ref': 'main', 'repo': {'full_name': 'intuitums/e'}}}
        unrelated = {**matching, 'number': 2, 'merge_commit_sha': 'b' * 40}
        comments = [
            {'id': 3, 'user': {'login': 'someone'}, 'body': MARKER},
            {'id': 4, 'user': {'login': 'github-actions[bot]'}, 'body': MARKER},
        ]
        with patch('pr_comment.api', side_effect=[[unrelated, matching], comments, {}]) as api:
            report(jobs(), 'finished', 'intuitums/e', 'https://github.com', '100', '1')
        self.assertEqual(api.call_args.args[1], 'issues/comments/4')
        self.assertEqual(api.call_args.args[3], 'PATCH')
        self.assertEqual(api.call_count, 3)

    def test_late_stage_cannot_replace_finished_comment(self):
        pr = {'number': 1, 'merged_at': 'today', 'merge_commit_sha': 'a' * 40,
              'base': {'ref': 'main', 'repo': {'full_name': 'intuitums/e'}}}
        comment = {'id': 4, 'user': {'login': 'github-actions[bot]'},
                   'body': MARKER + '\n<!-- run:100 attempt:1 stage:2 -->'}
        with patch('pr_comment.api', side_effect=[[pr], [comment]]) as api:
            report(jobs(), 'building', 'intuitums/e', 'https://github.com', '100', '1')
        self.assertEqual(api.call_count, 2)

    def test_processing_timeout_has_resume_instructions(self):
        needs = jobs('failure')
        needs['npm']['outputs'] = {'failure': 'processing'}
        body = render(needs, 'finished', 'url')
        self.assertIn('npm processing timed out', body)
        self.assertIn('rerun failed jobs to resume verification', body)

    def test_linux_failure_is_reported_before_skipped_publication(self):
        needs = jobs('skipped')
        needs['build-linux'] = {'result': 'failure'}
        body = render(needs, 'finished', 'url')
        self.assertIn('Linux build failure', body)
        self.assertNotIn('Incomplete', body)

    def test_direct_push_has_no_pr_comment(self):
        with patch('pr_comment.api', return_value=[]) as api:
            report(jobs(), 'finished', 'intuitums/e', 'https://github.com', '100', '1')
        self.assertEqual(api.call_count, 1)
