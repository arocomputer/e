"""Exercise staged checks and hook installation against disposable Git indexes."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

CHECK = Path(__file__).with_name('check.py').resolve()
INSTALL = Path(__file__).with_name('install.py').resolve()
HOOK = CHECK.parents[2] / '.githooks/pre-commit'


class HookTests(unittest.TestCase):
    def setUp(self):
        """Keep fixtures independent of personal hooks and Git identity settings."""
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.env = {**os.environ, 'GIT_CONFIG_GLOBAL': os.devnull, 'GIT_CONFIG_NOSYSTEM': '1'}
        self.run_command('git', 'init', '-q')
        self.run_command('git', 'config', 'user.name', 'Test')
        self.run_command('git', 'config', 'user.email', 'test@localhost')
        self.write('Cargo.toml', '[package]\nname = "fixture"\nversion = "0.1.0"\nedition = "2021"\n')
        self.run_command('git', 'add', '.')
        self.run_command('git', 'commit', '-qm', 'fixture')

    def run_command(self, *args):
        """Run tools inside the fixture without changing the test runner's directory."""
        return subprocess.run(args, cwd=self.root, env=self.env, capture_output=True, text=True, check=True)

    def write(self, name, content):
        """Create nested fixture files with exact content."""
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def check(self, *args):
        """Return a failed check for assertions instead of raising."""
        return subprocess.run(['python3', str(CHECK), *args], cwd=self.root, env=self.env, capture_output=True, text=True)

    def test_staged_rust_is_checked_without_touching_unstaged_edits(self):
        path = self.write('src/file with spaces.rs', 'fn main() {}\n')
        self.run_command('git', 'add', '.')
        path.write_text('fn main( ){ }\n')
        self.assertEqual(self.check().returncode, 0)
        self.assertEqual(path.read_text(), 'fn main( ){ }\n')
        self.run_command('git', 'add', '.')
        path.write_text('fn main() {}\n')
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('stage them again', result.stderr)
        self.assertEqual(self.run_command('git', 'show', ':src/file with spaces.rs').stdout, 'fn main( ){ }\n')

    def test_docs_whitespace_and_conflict_markers_fail(self):
        for content in ['text  \n', '<' * 7 + ' ours\ntext\n' + '=' * 7 + '\nother\n' + '>' * 7 + ' theirs\n']:
            self.write('guide.md', content)
            self.run_command('git', 'add', '.')
            self.assertNotEqual(self.check().returncode, 0)

    def test_ci_checks_committed_diff_even_with_a_clean_index(self):
        self.write('guide.md', 'text  \n')
        self.run_command('git', 'add', '.')
        self.run_command('git', 'commit', '-qm', 'bad whitespace')
        self.assertNotEqual(self.check('HEAD^').returncode, 0)

    def test_existing_precommit_is_preserved_and_run(self):
        previous = self.write('previous/pre-commit', '#!/bin/sh\nexit 23\n')
        previous.chmod(0o755)
        self.run_command('git', 'config', 'core.hooksPath', str(previous.parent))
        self.run_command('python3', str(INSTALL))
        self.run_command('python3', str(INSTALL))
        self.assertEqual(self.run_command('git', 'config', '--worktree', '--get', 'e.previousPreCommit').stdout.strip(), str(previous))
        result = subprocess.run(['sh', str(HOOK)], cwd=self.root, env=self.env, capture_output=True)
        self.assertEqual(result.returncode, 23)

    def test_other_hooks_are_never_silently_disabled(self):
        hook = self.write('previous/pre-push', '#!/bin/sh\nexit 0\n')
        hook.chmod(0o755)
        self.run_command('git', 'config', 'core.hooksPath', str(hook.parent))
        result = subprocess.run(['python3', str(INSTALL)], cwd=self.root, env=self.env, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.run_command('git', 'config', '--get', 'core.hooksPath').stdout.strip(), str(hook.parent))
