#!/usr/bin/env python3
"""Check whitespace, conflict markers, and Rust formatting without modifying files."""
import difflib
import re
import subprocess
import sys
from pathlib import PurePosixPath


def git(*args):
    """Read Git objects, retaining filenames and blob bytes exactly."""
    return subprocess.check_output(['git', *args])


def check(base=None):
    """Inspect the index locally, or the committed CI diff against its base."""
    diff = [base, 'HEAD'] if base else ['--cached']
    subprocess.run(['git', 'diff', '--check', *diff], check=True)
    paths = git('diff', '--name-only', '-z', '--diff-filter=ACMR', *diff).split(b'\0')
    prefix = 'HEAD:' if base else ':'
    failed = False
    for raw in paths:
        if not raw.endswith(b'.rs'):
            continue
        path = raw.decode('utf-8', errors='surrogateescape')
        # Use the nearest committed/staged manifest, including sdk and fuzz crates.
        edition = None
        for parent in PurePosixPath(path).parents:
            manifest = prefix + str(parent / 'Cargo.toml')
            result = subprocess.run(['git', 'show', manifest], capture_output=True)
            if result.returncode == 0:
                match = re.search(rb'^edition\s*=\s*"(\d+)"', result.stdout, re.MULTILINE)
                if match:
                    edition = match.group(1).decode()
                    break
        if edition is None:
            raise RuntimeError(f'Cannot determine Rust edition for {path}')
        print(f'Checking staged Rust: {path}' if not base else f'Checking Rust: {path}', flush=True)
        source = git('show', prefix + path)
        result = subprocess.run(['rustfmt', '--edition', edition,
                                 '--config', 'skip_children=true'], input=source, stdout=subprocess.PIPE)
        # rustfmt's stdin check mode can report differences with a zero exit status.
        if result.returncode != 0 or result.stdout != source:
            failed = True
            if result.returncode == 0:
                sys.stderr.writelines(difflib.unified_diff(
                    source.decode().splitlines(keepends=True),
                    result.stdout.decode().splitlines(keepends=True),
                    fromfile=path, tofile=path + ' (formatted)'))
    if failed:
        print('Format the reported files, review the changes, and stage them again.', file=sys.stderr)
    return int(failed)


if __name__ == '__main__':
    if len(sys.argv) > 2:
        sys.exit('usage: check.py [base-commit]')
    try:
        sys.exit(check(sys.argv[1] if len(sys.argv) == 2 else None))
    except (subprocess.CalledProcessError, RuntimeError, FileNotFoundError) as error:
        sys.exit(str(error))
