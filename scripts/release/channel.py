#!/usr/bin/env python3
"""Promote a verified beta using its repository's latest release, without pointer releases."""
import json
import subprocess
import sys
from identity import identity, version_key


def advance(tag):
    """Keep the latest beta monotonic when retrying an older publication."""
    release = identity(tag)
    if release['channel'] != 'beta':
        return
    repository = release['repository']
    # Listing succeeds even before the first latest release exists; API failures must stop.
    pages = json.loads(subprocess.check_output(
        ['gh', 'api', '--paginate', f'repos/{repository}/releases?per_page=100', '--slurp'], text=True))
    published = [r for page in pages for r in page if not r['draft'] and not r['prerelease']]
    if any(version_key(r['tag_name']) > version_key(tag) for r in published):
        return
    subprocess.run(['gh', 'release', 'edit', tag, '--repo', repository, '--latest'], check=True)


if __name__ == '__main__':
    advance(sys.argv[1])
