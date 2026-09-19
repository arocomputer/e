"""Stable publication requires explicit beta selection and unchanged build inputs."""
import unittest
from unittest.mock import patch
from promotion import verify

SHA = 'abcdef012345' + 'a' * 28
TAG = 'v0.0.0-beta-12'


class PromotionTests(unittest.TestCase):
    def check(self, *, message=None, changed='', draft=False, deployed=True, source=SHA):
        """Supply release evidence and retain the actual promotion validation."""
        def git(*args):
            if args[0] == 'cat-file':
                return 'tag'
            return (message if message is not None else f'Release 1.2.3\n\nBeta: {TAG}') if args[0] == 'for-each-ref' else changed
        def gh(*args):
            if args[0] == 'release':
                return {'isDraft': draft, 'body': f'Source: https://github.com/arocomputer/e/commit/{source}'}
            if args[-1].endswith('/statuses'):
                return [{'state': 'success' if deployed else 'failure'}]
            return [[{'id': 12, 'payload': {'tag': TAG}}]]
        with patch('promotion.git', side_effect=git), patch('promotion.gh', side_effect=gh), \
                patch('promotion.subprocess.run') as ancestor:
            result = verify('v1.2.3', 'b' * 40)
        ancestor.assert_called_once_with(['git', 'merge-base', '--is-ancestor', SHA, 'b' * 40], check=True)
        return result

    def test_promotes_the_explicit_verified_beta(self):
        self.assertEqual(self.check(), TAG)

    def test_code_or_build_input_changes_require_another_beta(self):
        for path in ['src/main.rs', 'Cargo.lock', 'build.rs', '.github/workflows/release.yml']:
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, 'publish another beta'):
                self.check(changed=path)

    def test_requires_one_matching_beta_selection(self):
        for message in ['Release 1.2.3', f'Beta: {TAG}\nBeta: {TAG}', 'Beta: v0.0.0-beta-1']:
            with self.subTest(message=message), self.assertRaises(ValueError):
                self.check(message=message)

    def test_unpublished_or_unverified_betas_cannot_promote(self):
        # The beta release body names the source commit, and the verified beta
        # deployment must exist for exactly that commit; a body with no matching
        # deployment is rejected the same as a draft or a failed deployment.
        for options in [{'draft': True}, {'deployed': False}]:
            with self.subTest(options=options), self.assertRaises(ValueError):
                self.check(**options)

    def test_real_tag_allows_changelog_only_and_rejects_changed_source(self):
        import os
        from pathlib import Path
        import subprocess
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            env = dict(os.environ, GIT_AUTHOR_NAME='Release test', GIT_AUTHOR_EMAIL='release@example.invalid',
                       GIT_COMMITTER_NAME='Release test', GIT_COMMITTER_EMAIL='release@example.invalid')
            def git(*args):
                return subprocess.check_output(['git', '-C', tmp, *args], env=env, text=True).strip()
            git('init', '-q')
            Path(tmp, 'source.rs').write_text('tested source')
            git('add', '.')
            git('commit', '-qm', 'Beta source')
            source = git('rev-parse', 'HEAD')
            beta = 'v0.0.0-beta-12'
            Path(tmp, 'CHANGELOG.md').write_text('Release notes')
            git('add', '.')
            git('commit', '-qm', 'Release notes')
            git('tag', '-a', 'v1.2.3', '-m', f'Release\n\nBeta: {beta}')
            # Run real git inspection and ancestry checks in the fixture repository.
            run = subprocess.run
            def ancestry(command, **kwargs):
                return run(['git', '-C', tmp, *command[1:]], **kwargs)
            head = git('rev-parse', 'HEAD')
            with patch('promotion.git', side_effect=git), patch('promotion.gh', side_effect=[
                    {'isDraft': False, 'body': f'Source: https://github.com/arocomputer/e/commit/{source}'},
                    [[{'id': 1, 'payload': {'tag': beta}}]], [{'state': 'success'}]]), \
                    patch('promotion.subprocess.run', side_effect=ancestry):
                self.assertEqual(verify('v1.2.3', head), beta)
            Path(tmp, 'source.rs').write_text('untested source')
            git('add', '.')
            git('commit', '-qm', 'Change source')
            head = git('rev-parse', 'HEAD')
            git('tag', '-fa', 'v1.2.3', '-m', f'Release\n\nBeta: {beta}')
            with patch('promotion.git', side_effect=git), patch('promotion.gh', return_value={
                    'isDraft': False, 'body': f'Source: https://github.com/arocomputer/e/commit/{source}'}), \
                    patch('promotion.subprocess.run', side_effect=ancestry):
                with self.assertRaisesRegex(ValueError, 'source.rs'):
                    verify('v1.2.3', head)
