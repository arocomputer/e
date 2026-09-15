"""Publishing a formula is repeatable and cannot roll an existing tap backward."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BrewPublishing(unittest.TestCase):
    def test_publish_retry_and_older_tag(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            remote, tap = root / "remote.git", root / "tap"

            def run(*args, **kwargs):
                return (
                    subprocess.check_output(args, stderr=subprocess.DEVNULL, **kwargs)
                    .decode()
                    .strip()
                )

            run("git", "init", "--bare", "--initial-branch=main", str(remote))
            run("git", "clone", str(remote), str(tap))
            (tap / "README.md").write_text("tap\n")
            run("git", "-C", str(tap), "add", ".")
            run(
                "git",
                "-C",
                str(tap),
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "init",
            )
            run("git", "-C", str(tap), "push", "origin", "main")
            formula = root / "e.rb"

            def publish(version):
                formula.write_text(f'class E < Formula\n  version "{version}"\nend\n')
                run(
                    "sh",
                    str(ROOT / "scripts/packaging/publish-brew.sh"),
                    str(formula),
                    str(tap),
                    env=dict(os.environ, TAG="v" + version),
                )

            publish("1.2.3")
            head = run("git", "-C", str(tap), "rev-parse", "HEAD")
            publish("1.2.3")
            publish("1.2.2")
            self.assertEqual(head, run("git", "-C", str(tap), "rev-parse", "HEAD"))
            self.assertIn("1.2.3", (tap / "Formula/e.rb").read_text())
