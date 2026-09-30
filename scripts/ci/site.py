"""Build the pinned website renderer against this checkout's actual guides."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def build(source):
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    env = dict(os.environ, E_DOCS_PATH=str(ROOT), E_DOCS_REF=commit)
    for command in (['npm', 'ci'], ['npm', 'test'], ['npm', 'run', 'typecheck'], ['npm', 'run', 'build']):
        subprocess.run(command, cwd=source, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, help='Local website checkout for development')
    args = parser.parse_args()
    if args.source:
        build(args.source.resolve())
        return
    spec = json.loads((ROOT / '.github/site-source.json').read_text())
    if spec['repository'] != 'arocomputer/web' or not re.fullmatch('[0-9a-f]{40}', spec['commit']):
        raise ValueError('website source must be the pinned official repository')
    with tempfile.TemporaryDirectory(prefix='e-site-') as temp:
        source = Path(temp)
        subprocess.run(['git', 'init', '-q', str(source)], check=True)
        subprocess.run(['git', 'fetch', '--depth=1', 'https://github.com/arocomputer/web.git', spec['commit']], cwd=source, check=True)
        subprocess.run(['git', 'checkout', '--detach', 'FETCH_HEAD'], cwd=source, check=True)
        build(source)


if __name__ == '__main__':
    main()
