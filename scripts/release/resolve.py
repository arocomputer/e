#!/usr/bin/env python3
"""Select a trusted source commit and emit workflow outputs without publishing anything."""
import json
import os
import re
import subprocess
import sys
from pathlib import Path
import tomllib
from identity import identity
from promotion import verify

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "ci"))
from changes import classify


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def resolve():
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    kind = os.environ['GITHUB_EVENT_NAME']
    mode = event.get('inputs', {}).get('action', 'build')
    if kind == 'workflow_run':
        run = event['workflow_run']
        assert run['conclusion'] == 'success' and run['head_branch'] == 'main' and run['event'] == 'push'
        assert run['head_repository']['full_name'] == os.environ['GITHUB_REPOSITORY']
        sha, channel = run['head_sha'], 'dev'
    elif kind == 'push':
        sha, channel = git('rev-parse', 'HEAD'), 'production'
    elif mode == 'retry':
        tag = event['inputs']['tag']
        release = identity(tag)
        assert release['channel'] in ('production', 'beta'), 'Rerun the original Actions run to retry a dev build'
        metadata = json.loads(subprocess.check_output(
            ['gh', 'release', 'view', tag, '--repo', release['repository'], '--json', 'isDraft,targetCommitish,body'], text=True))
        # Draft releases may not have a tag until they are published.
        if release['channel'] == 'beta':
            source = re.search(r'^Source: https://github.com/intuitums/e/commit/([a-f0-9]{40})$', metadata['body'], re.M)
            assert source, 'Beta release is missing its source commit'
            ref = source[1]
        else:
            ref = metadata['targetCommitish'] if metadata['isDraft'] else f'refs/tags/{tag}'
        sha = git('rev-parse', '--verify', f'{ref}^{{commit}}')
        subprocess.run(['git', 'merge-base', '--is-ancestor', sha, 'origin/main'], check=True)
        return release | {'sha': sha, 'tag': tag, 'mode': 'recover' if metadata['isDraft'] else 'retry'}
    else:
        ref = event.get('inputs', {}).get('commit') or 'origin/main'
        sha, channel = git('rev-parse', '--verify', f'{ref}^{{commit}}'), 'beta'
    # Only reviewed commits from main may receive publishing credentials.
    subprocess.run(['git', 'merge-base', '--is-ancestor', sha, 'origin/main'], check=True)
    # The version moved to [workspace.package] with the crates split; older
    # commits keep it under [package].
    manifest = tomllib.loads(git('show', f'{sha}:Cargo.toml'))
    base = (manifest.get('workspace', {}).get('package') or manifest['package'])['version']
    version = base if channel == 'production' else f'0.0.0-{channel}-{os.environ["GITHUB_RUN_NUMBER"]}'
    if channel == 'production':
        assert os.environ['GITHUB_REF_NAME'] == f'v{version}', 'production tag must match manifest'
        verify(f'v{version}', sha)
    mode = 'build'
    if kind == 'workflow_run':
        paths = git('diff-tree', '--no-commit-id', '--name-only', '-r', sha).splitlines()
        if paths and not classify(paths)['publish']:
            mode = 'skip'
    return identity(version) | {'sha': sha, 'tag': f'v{version}', 'mode': mode}


if __name__ == '__main__':
    result = resolve()
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        for key, value in result.items():
            output.write(f'{key}={value}\n')
