"""Exercise maintenance decisions without GitHub writes or network access."""
import copy
import unittest
from unittest.mock import patch
import cleanup
import report
import settings
import subprocess


class CacheTests(unittest.TestCase):
    def test_only_closed_pr_merge_ref_is_deleted_after_all_pages_are_read(self):
        event = {'action': 'closed', 'number': 42, 'pull_request': {'state': 'closed'}}
        caches = [{'actions_caches': [{'id': 1, 'ref': 'refs/pull/42/merge'},
                                    {'id': 2, 'ref': 'refs/heads/main'}]},
                  {'actions_caches': [{'id': 3, 'ref': 'refs/pull/43/merge'},
                                    {'id': 4, 'ref': 'refs/pull/42/merge'}]}]
        with patch('cleanup.pages', return_value=caches) as read, patch('cleanup.request') as delete:
            cleanup.cleanup('owner/repo', event)
            self.assertIn('ref=refs%2Fpull%2F42%2Fmerge', read.call_args.args[0])
            self.assertEqual([call.args for call in delete.call_args_list], [
                ('repos/owner/repo/actions/caches/1', 'DELETE'),
                ('repos/owner/repo/actions/caches/4', 'DELETE')])

    def test_open_pr_cannot_delete_caches(self):
        with patch('cleanup.pages') as read, self.assertRaises(ValueError):
            cleanup.cleanup('owner/repo', {'action': 'opened', 'number': 42,
                                          'pull_request': {'state': 'open'}})
        read.assert_not_called()


class ReportTests(unittest.TestCase):
    def setUp(self):
        self.event = {'repository': {'default_branch': 'main'}, 'workflow_run': {
            'id': 7, 'workflow_id': 12, 'name': 'fuzz', 'event': 'schedule',
            'head_branch': 'main', 'head_repository': {'full_name': 'owner/repo'},
            'conclusion': 'failure', 'run_number': 3, 'html_url': 'https://github.com/owner/repo/actions/runs/7'}}
        self.issue = {'number': 2, 'state': 'open', 'body': '<!-- scheduled-workflow:12 --> old failure',
                      'user': {'login': 'github-actions[bot]'}}

    def run_report(self, issues, latest=7):
        calls = []
        def api(path, method='GET', body=None):
            if method == 'GET':
                return {'workflow_runs': [{'id': latest}]}
            calls.append((path, method, body))
        with patch('report.request', side_effect=api), patch('report.pages', return_value=[issues]):
            report.report('owner/repo', self.event)
        return calls

    def test_first_failure_creates_one_issue_without_labels(self):
        calls = self.run_report([])
        self.assertEqual(calls[0][1], 'POST')
        self.assertNotIn('labels', calls[0][2])

    def test_repeated_failure_updates_and_closed_failure_reopens_same_issue(self):
        for state in ('open', 'closed'):
            self.issue['state'] = state
            calls = self.run_report([self.issue])
            self.assertEqual(len(calls), 1)
            self.assertEqual(calls[0][:2], ('repos/owner/repo/issues/2', 'PATCH'))
            self.assertEqual(calls[0][2]['state'], 'open')

    def test_recovery_closes_issue_and_healthy_repo_stays_quiet(self):
        self.event['workflow_run']['conclusion'] = 'success'
        self.assertEqual(self.run_report([self.issue])[0][2]['state'], 'closed')
        self.assertEqual(self.run_report([]), [])

    def test_manual_pr_fork_cancelled_and_outdated_runs_do_not_write(self):
        original = copy.deepcopy(self.event)
        for field, value in [('event', 'pull_request'), ('event', 'workflow_dispatch'),
                             ('head_branch', 'feature'), ('conclusion', 'cancelled'),
                             ('head_repository', {'full_name': 'fork/repo'})]:
            self.event = copy.deepcopy(original)
            self.event['workflow_run'][field] = value
            with patch('report.request') as api:
                report.report('owner/repo', self.event)
            api.assert_not_called()
        self.event = original
        self.assertEqual(self.run_report([self.issue], latest=8), [])

    def test_delayed_completion_of_previous_run_attempt_stays_quiet(self):
        self.event['workflow_run']['run_attempt'] = 1
        with patch('report.request', return_value={'workflow_runs': [{'id': 7, 'run_attempt': 2}]}) as api, \
                patch('report.pages') as issues:
            report.report('owner/repo', self.event)
        self.assertEqual(api.call_count, 1)
        issues.assert_not_called()


class SettingsTests(unittest.TestCase):
    def test_required_checks_preserve_other_checks_and_their_app_ids(self):
        current = {'strict': False, 'checks': [{'context': 'other', 'app_id': 123}]}
        desired = settings.required_checks(current)
        self.assertEqual(desired['checks'], [{'context': 'other', 'app_id': 123}, {'context': 'ready'}])
        self.assertTrue(desired['strict'])
        self.assertEqual(settings.required_checks(desired), desired)

    def test_apply_changes_only_checks_without_replacing_review_policy(self):
        repo = {'default_branch': 'main', 'delete_branch_on_merge': True,
                'security_and_analysis': {name: {'status': 'enabled'} for name in
                 ('secret_scanning', 'secret_scanning_push_protection')}}
        policy = {'required_status_checks': {'strict': False, 'contexts': ['other']},
                  'required_pull_request_reviews': {'required_approving_review_count': 2}}
        with patch('settings.request', side_effect=[repo, policy, None]) as api:
            settings.configure('owner/repo', apply=True)
        self.assertEqual(api.call_args.args[0], 'repos/owner/repo/branches/main/protection/required_status_checks')
        self.assertEqual(api.call_args.args[1], 'PATCH')
        self.assertNotIn('required_pull_request_reviews', api.call_args.args[2])

    def test_admin_denial_stops_before_any_write(self):
        repo = {'default_branch': 'main', 'delete_branch_on_merge': False}
        denied = subprocess.CalledProcessError(1, ['gh'], stderr='HTTP 403')
        with patch('settings.request', side_effect=[repo, denied]) as api:
            with self.assertRaises(subprocess.CalledProcessError):
                settings.configure('owner/repo', apply=True)
        self.assertEqual(api.call_count, 2)
