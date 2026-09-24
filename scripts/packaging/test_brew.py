"""Publishing a formula is repeatable and cannot roll an existing tap backward."""

import json
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
            (tap / "Formula").mkdir()
            (tap / "Formula/e.rb").write_text('class E < Formula\n  version "1.2.0"\nend\n')
            (tap / "formula_renames.json").write_text('{"another-old-name":"another-formula"}\n')
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
            formula = root / "ulo.rb"

            def publish(version):
                formula.write_text(f'class Ulo < Formula\n  version "{version}"\nend\n')
                run(
                    "sh",
                    str(ROOT / "scripts/packaging/publish-brew.sh"),
                    str(formula),
                    str(tap),
                    env=dict(os.environ, TAG="v" + version),
                )

            publish("1.2.3")
            self.assertFalse((tap / "Formula/e.rb").exists())
            self.assertEqual(json.loads((tap / "formula_renames.json").read_text()), {
                "e": "ulo", "another-old-name": "another-formula",
            })
            head = run("git", "-C", str(tap), "rev-parse", "HEAD")
            publish("1.2.3")
            publish("1.2.2")
            self.assertEqual(head, run("git", "-C", str(tap), "rev-parse", "HEAD"))
            self.assertIn("1.2.3", (tap / "Formula/ulo.rb").read_text())
