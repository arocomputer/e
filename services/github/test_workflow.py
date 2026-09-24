"""Execute the workflow's authorization step with simulated GitHub responses."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


class AuthorizationTests(unittest.TestCase):
    """Only repository writers may start a turn, on either comment event."""

    def authorize(self, permission, event_kind):
        """Run the actual shell step without contacting GitHub."""
        workflow = Path(__file__).with_name("ulo.yml").read_text()
        script = ""
        if "- name: Authorize commenter\n" in workflow:
            step = workflow.split("- name: Authorize commenter\n", 1)[1].split("\n      - ", 1)[0]
            script = textwrap.dedent(step.split("run: |\n", 1)[1])
        # Removing the gate is a successful no-op, which must fail denial tests.
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            gh = root / "gh"
            gh.write_text('#!/bin/sh\nprintf "%s\\n" "$PERMISSION"\nexit "$API_STATUS"\n')
            gh.chmod(0o755)
            event = root / "event.json"
            event.write_text(json.dumps({"comment": {"user": {"login": "caller"}}, event_kind: {"number": 42}}))
            output = root / "output"
            output.touch()
            result = subprocess.run(
                ["sh", "-c", script], capture_output=True, text=True,
                env={**os.environ, "PATH": f"{root}:{os.environ['PATH']}",
                     "PERMISSION": permission, "API_STATUS": "1" if permission == "api-error" else "0",
                     "GITHUB_REPOSITORY": "owner/repo", "GITHUB_EVENT_PATH": str(event),
                     "GITHUB_OUTPUT": str(output)},
            )
            return result.returncode, output.read_text()

    def test_readers_and_failed_permission_lookups_cannot_start_a_turn(self):
        for permission in ["read", "triage", "none", "", "api-error"]:
            with self.subTest(permission=permission):
                status, output = self.authorize(permission, "issue")
                self.assertNotEqual(status, 0)
                self.assertEqual(output, "")

    def test_writers_get_the_reply_number_for_both_comment_events(self):
        for event in ["issue", "pull_request"]:
            for permission in ["write", "maintain", "admin"]:
                with self.subTest(event=event, permission=permission):
                    status, output = self.authorize(permission, event)
                    self.assertEqual(status, 0)
                    self.assertEqual(output, "number=42\n")


if __name__ == "__main__":
    unittest.main()
