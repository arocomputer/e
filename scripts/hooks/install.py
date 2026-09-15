#!/usr/bin/env python3
"""Enable repository hooks for this worktree while retaining an existing pre-commit hook."""
import os
import subprocess
from pathlib import Path


def git(*args):
    """Read or set Git configuration without invoking a shell."""
    return subprocess.check_output(['git', *args], text=True).strip()


def install():
    """Do not replace other active hooks; each linked worktree opts into its own checkout."""
    root = Path(git('rev-parse', '--show-toplevel'))
    hooks = root / '.githooks'
    configured = subprocess.run(['git', 'config', '--path', '--get', 'core.hooksPath'], capture_output=True, text=True)
    previous = Path(configured.stdout.strip()) if configured.returncode == 0 else Path(git('rev-parse', '--git-path', 'hooks'))
    previous = previous.resolve()
    if previous != hooks:
        others = [p.name for p in previous.glob('*') if p.is_file() and os.access(p, os.X_OK)
                  and p.name != 'pre-commit' and not p.name.endswith('.sample')]
        if others:
            raise SystemExit('Existing hooks need manual integration before setup: ' + ', '.join(sorted(others)))
    # Git otherwise shares local configuration across linked worktrees.
    git('config', '--local', 'extensions.worktreeConfig', 'true')
    if previous != hooks:
        hook = previous / 'pre-commit'
        if hook.is_file() and os.access(hook, os.X_OK):
            git('config', '--worktree', 'e.previousPreCommit', str(hook))
    git('config', '--worktree', 'core.hooksPath', '.githooks')
    print('Repository hooks enabled for this worktree.')


if __name__ == '__main__':
    install()
