#!/usr/bin/env python3
"""Refuse to publish a release without its verified executable archives and metadata."""
import json
import subprocess
import sys


REQUIRED = {
    f'e-{target}.tar.gz' for target in (
        'aarch64-apple-darwin', 'x86_64-apple-darwin',
        'aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu',
    )
} | {'checksums.txt', 'e-sbom.cdx.json', 'build.json', 'release.json'}


def verify(tag):
    release = json.loads(subprocess.check_output(
        ['gh', 'release', 'view', tag, '--json', 'assets'], text=True))
    assets = {asset['name'] for asset in release['assets'] if asset['size'] > 0}
    missing = REQUIRED - assets
    if missing:
        raise ValueError('Release assets missing or empty: ' + ', '.join(sorted(missing)))


if __name__ == '__main__':
    verify(sys.argv[1])
