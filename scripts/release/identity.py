#!/usr/bin/env python3
"""Resolve one release identity for builds, package publication, and channel pointers."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib

VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-pr-([1-9][0-9]*))?')


def identity(version):
    """Reject unsupported version forms rather than guessing their update channel."""
    match = VERSION.fullmatch(version.removeprefix('v'))
    if not match:
        raise ValueError('Expected X.Y.Z or 0.0.0-pr-NUMBER')
    channel = 'pr' if match[4] else 'production'
    title = '.'.join(match.group(1, 2, 3))
    if channel == 'pr':
        title += f' · PR {match[4]}'
    return {'version': version.removeprefix('v'), 'channel': channel, 'title': title,
            'command': 'ulo' if channel == 'production' else 'ulo-pr',
            'npm_tag': 'latest' if channel == 'production' else 'pr',
            'repository': 'arocomputer/ulo'}


def version_key(version):
    """Order pre-release versions by their numeric sequence."""
    match = VERSION.fullmatch(version.removeprefix('v'))
    if not match:
        raise ValueError('Invalid release version')
    return (*map(int, match.group(1, 2, 3)), int(match[4] or 0))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--channel', choices=['pr', 'production'], required=True)
    parser.add_argument('--sequence', type=int, default=0)
    parser.add_argument('--tag')
    args = parser.parse_args()
    manifest = tomllib.loads(Path('Cargo.toml').read_text())
    base = (manifest.get('workspace', {}).get('package') or manifest['package'])['version']
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    version = base if args.channel == 'production' else f'0.0.0-{args.channel}-{args.sequence}'
    if args.tag and args.tag != f'v{version}':
        raise SystemExit('Tag does not match the channel version')
    result = identity(version) | {'commit': commit, 'tag': f'v{version}'}
    print(json.dumps(result))
