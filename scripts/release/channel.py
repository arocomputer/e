#!/usr/bin/env python3
"""Advance a preview channel only after all installers publish, never on an older retry."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from identity import identity, version_key


def gh(*args):
    return subprocess.check_output(['gh', *args], text=True)


def advance(tag):
    release = identity(tag)
    if release['channel'] == 'stable':
        return
    pointer = f'channel-{release["channel"]}'
    with tempfile.TemporaryDirectory() as tmp:
        version = Path(tmp) / 'version.txt'
        # A missing pointer is normal on first publication; other failures must stop.
        releases = json.loads(gh('api', '--paginate', 'repos/{owner}/{repo}/releases?per_page=100', '--slurp'))
        existing = next((r for page in releases for r in page if r['tag_name'] == pointer), None)
        if existing:
            gh('release', 'download', pointer, '--pattern', 'version.txt', '--dir', tmp)
            if version_key(version.read_text().strip()) > version_key(tag):
                return
        else:
            gh('release', 'create', pointer, '--prerelease', '--latest=false', '--title', f'e {release["channel"]}', '--notes', 'Installer channel pointer. Versioned builds are separate prereleases.')
        version.write_text(tag + '\n')
        gh('release', 'upload', pointer, str(version), '--clobber')


if __name__ == '__main__':
    advance(sys.argv[1])
