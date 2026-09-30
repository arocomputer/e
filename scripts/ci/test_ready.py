"""The ready job is the one required check, so it must wait on every ci job."""
from pathlib import Path
import re
import unittest
import json
from changes import JOBS, classify, job_plan
from ready import verify


class ReadyTests(unittest.TestCase):
    def test_ready_needs_every_job(self):
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/ci.yml').read_text()
        jobs = set(re.findall(r'^  ([a-z0-9-]+):$', workflow[workflow.index('\njobs:'):], re.M)) - {'ready'}
        ready = workflow[workflow.index('\n  ready:'):]
        needs = set(re.search(r'needs: \[([^\]]*)\]', ready).group(1).replace(' ', '').split(','))
        self.assertEqual(needs, jobs)

    def fixture(self):
        plan = job_plan(classify(['crates/cli/tests/rpc.rs']), 'pull_request')
        needs = {'changes': {'result': 'success', 'outputs': {'plan': json.dumps(plan)}}}
        needs.update({job: {'result': 'success' if required else 'skipped'} for job, required in plan.items()})
        return needs

    def test_selected_skips_cancellation_and_failed_selection_are_rejected(self):
        for job, result in [('test-linux', 'skipped'), ('test-linux', 'cancelled'), ('changes', 'failure')]:
            needs = self.fixture()
            needs[job]['result'] = result
            with self.assertRaises(ValueError):
                verify(needs)

    def test_intended_skips_pass_and_malformed_selection_fails(self):
        needs = self.fixture()
        verify(needs)
        for plan in ({}, dict.fromkeys(JOBS, 'false')):
            needs['changes']['outputs']['plan'] = json.dumps(plan)
            with self.assertRaises(ValueError):
                verify(needs)

    def test_housekeeping_has_no_native_or_container_build(self):
        for path in ('.github/workflows/cache-cleanup.yml', 'scripts/ci/report.py', '.github/infra-tools.json'):
            plan = job_plan(classify([path]), 'pull_request')
            self.assertEqual([job for job, enabled in plan.items() if enabled], ['lint'])

    def test_runtime_tests_do_not_repeat_ui_and_packed_consumers(self):
        plan = job_plan(classify(['crates/cli/tests/rpc.rs']), 'pull_request')
        self.assertTrue(plan['test-linux'])
        self.assertTrue(plan['test-macos'])
        self.assertFalse(plan['ui'])
        self.assertFalse(plan['crates'])

    def test_guides_require_the_actual_website(self):
        self.assertTrue(job_plan(classify(['docs/guides/start/getting-started.md']), 'pull_request')['site'])


if __name__ == '__main__':
    unittest.main()
