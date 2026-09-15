#!/usr/bin/env python3
"""Resolve one release identity for builds, package publication, and channel pointers."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib

VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(dev|beta|pr)\.(0|[1-9][0-9]*)\.g([a-f0-9]{12}))?')


def identity(version):
    """Reject unsupported version forms rather than guessing their update channel."""
    match = VERSION.fullmatch(version.removeprefix('v'))
    if not match:
        raise ValueError('Expected X.Y.Z or X.Y.Z-{dev,beta,pr}.NUMBER.gCOMMIT')
    channel = match[4] or 'stable'
    return {'version': version.removeprefix('v'), 'channel': channel,
            'command': 'e' if channel == 'stable' else f'e-{channel}',
            'npm_tag': 'latest' if channel == 'stable' else channel}


def version_key(version):
    """Order versions within one channel, including numeric preview sequence numbers."""
    match = VERSION.fullmatch(version.removeprefix('v'))
    if not match:
        raise ValueError('Invalid release version')
    return (*map(int, match.group(1, 2, 3)), int(match[5] or 0))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--channel', choices=['dev', 'beta', 'pr', 'stable'], required=True)
    parser.add_argument('--sequence', type=int, default=0)
    parser.add_argument('--tag')
    args = parser.parse_args()
    base = tomllib.loads(Path('Cargo.toml').read_text())['package']['version']
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    version = base if args.channel == 'stable' else f'{base}-{args.channel}.{args.sequence}.g{commit[:12]}'
    if args.tag and args.tag != f'v{version}':
        raise SystemExit('Tag does not match Cargo.toml')
    result = identity(version) | {'commit': commit, 'tag': f'v{version}'}
    print(json.dumps(result))
