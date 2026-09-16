"""Protect the boundaries between documentation, native builds, and publication."""
import json
import os
import unittest
from pathlib import Path
import subprocess
import tempfile
from unittest.mock import patch

from changes import changed_paths, classify, main


class ChangesTests(unittest.TestCase):
    def test_readme_artwork_does_not_build_or_publish(self):
        gates = classify(['README.md', 'assets/readme.png', 'assets/readme-window.html'])
        self.assertTrue(gates['docs'])
        self.assertFalse(gates['build'])
        self.assertFalse(gates['publish'])
        self.assertFalse(gates['bench'])

    def test_unknown_paths_fail_open(self):
        gates = classify(['new-runtime/input.dat'])
        self.assertTrue(gates['build'])
        self.assertTrue(gates['publish'])

    def test_embedded_theme_is_runtime_data(self):
        gates = classify(['crates/core/themes/dark.json'])
        self.assertTrue(gates['build'])
        self.assertTrue(gates['publish'])
        self.assertTrue(gates['bench'])

    def test_guides_get_docs_without_duplicate_full_suite(self):
        gates = classify(['docs/guides/start/install.md'])
        self.assertTrue(gates['docs'])
        self.assertFalse(gates['build'])

    def test_crate_manifest_checks_linux_compatibility(self):
        gates = classify(['crates/sdk/Cargo.toml'])
        self.assertTrue(gates['lock'])
        self.assertTrue(gates['build'])

    def test_rpc_tests_do_not_require_performance_budgets(self):
        gates = classify(['crates/cli/tests/rpc.rs'])
        self.assertTrue(gates['build'])
        self.assertFalse(gates['bench'])

    def test_failed_file_listing_runs_every_check(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / 'output'
            with patch.dict(os.environ, GITHUB_OUTPUT=str(output)), \
                    patch('changes.changed_paths', side_effect=subprocess.CalledProcessError(1, 'gh')):
                main()
            self.assertEqual(dict(line.split('=') for line in output.read_text().splitlines()),
                             dict.fromkeys(classify([]), 'true'))

    def test_rename_out_of_runtime_still_checks_old_path(self):
        pages = [[{'filename': 'assets/old.json', 'previous_filename': 'crates/core/themes/old.json'}]]
        with patch.dict(os.environ, PR='1', GITHUB_REPOSITORY='intuitums/e'), \
                patch('changes.subprocess.check_output', return_value=json.dumps(pages)):
            self.assertTrue(classify(changed_paths())['build'])


if __name__ == '__main__':
    unittest.main()
