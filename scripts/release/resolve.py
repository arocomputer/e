#!/usr/bin/env python3
"""Select a trusted source commit and emit workflow outputs without publishing anything."""
import json
import os
import subprocess
import sys
from pathlib import Path
import tomllib
from identity import identity

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "ci"))
from changes import classify


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def resolve():
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    kind = os.environ['GITHUB_EVENT_NAME']
    mode = event.get('inputs', {}).get('action', 'build')
    if kind == 'push':
        sha, channel = git('rev-parse', 'HEAD'), 'production'
    elif mode == 'retry':
        tag = event['inputs']['tag']
        release = identity(tag)
        assert release['channel'] == 'production', 'Only production packages can be retried'
        metadata = json.loads(subprocess.check_output(
            ['gh', 'release', 'view', tag, '--repo', release['repository'], '--json', 'isDraft,targetCommitish'], text=True))
        ref = metadata['targetCommitish'] if metadata['isDraft'] else f'refs/tags/{tag}'
        sha = git('rev-parse', '--verify', f'{ref}^{{commit}}')
        subprocess.run(['git', 'merge-base', '--is-ancestor', sha, 'origin/main'], check=True)
        return release | {'sha': sha, 'tag': tag, 'mode': 'recover' if metadata['isDraft'] else 'retry'}
    else:
        ref = event.get('inputs', {}).get('commit') or 'origin/main'
        sha, channel = git('rev-parse', '--verify', f'{ref}^{{commit}}'), 'production'
    # Only reviewed commits from main may receive publishing credentials.
    subprocess.run(['git', 'merge-base', '--is-ancestor', sha, 'origin/main'], check=True)
    # The version moved to [workspace.package] with the crates split; older
    # commits keep it under [package].
    manifest = tomllib.loads(git('show', f'{sha}:Cargo.toml'))
    base = (manifest.get('workspace', {}).get('package') or manifest['package'])['version']
    version = base
    assert os.environ['GITHUB_REF_NAME'] == f'v{version}', 'production tag must match manifest'
    return identity(version) | {'sha': sha, 'tag': f'v{version}', 'mode': 'build'}


if __name__ == '__main__':
    result = resolve()
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        for key, value in result.items():
            output.write(f'{key}={value}\n')
