"""The ready job is the one required check, so it must wait on every ci job."""
from pathlib import Path
import re
import unittest


class ReadyTests(unittest.TestCase):
    def test_ready_needs_every_job(self):
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/ci.yml').read_text()
        jobs = set(re.findall(r'^  ([a-z0-9-]+):$', workflow[workflow.index('\njobs:'):], re.M)) - {'ready'}
        ready = workflow[workflow.index('\n  ready:'):]
        needs = set(re.search(r'needs: \[([^\]]*)\]', ready).group(1).replace(' ', '').split(','))
        self.assertEqual(needs, jobs)


if __name__ == '__main__':
    unittest.main()
