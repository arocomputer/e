#!/usr/bin/env python3
"""Require stable tags to name a verified beta with identical code and build inputs."""
import json
import re
import subprocess
import sys
from identity import identity


def git(*args):
    """Read promotion evidence from the fetched source repository."""
    return subprocess.check_output(['git', *args], text=True).strip()


def gh(*args):
    """Read published beta and deployment evidence without changing GitHub state."""
    return json.loads(subprocess.check_output(['gh', *args], text=True))


def verify(tag, sha):
    """Allow only changelog edits after the explicitly selected, successfully deployed beta."""
    stable = identity(tag)
    if stable['channel'] != 'stable':
        raise ValueError('Promotion requires a stable tag')
    if git('cat-file', '-t', f'refs/tags/{tag}') != 'tag':
        raise ValueError('Stable releases require an annotated tag naming the tested beta')
    message = git('for-each-ref', '--format=%(contents)', f'refs/tags/{tag}')
    selected = re.findall(r'^Beta: (v\S+)$', message, re.M)
    if len(selected) != 1:
        raise ValueError('Annotate the stable tag with exactly one Beta: vX.Y.Z-beta.N.gCOMMIT line')
    beta_tag = selected[0]
    beta = identity(beta_tag)
    if beta['channel'] != 'beta' or beta['version'].split('-')[0] != stable['version']:
        raise ValueError('Selected beta must have the same base version as stable')
    release = gh('release', 'view', beta_tag, '--repo', 'intuitums/e-beta', '--json', 'isDraft,body')
    source = re.findall(r'^Source: https://github.com/intuitums/e/commit/([a-f0-9]{40})$', release['body'], re.M)
    if release['isDraft'] or len(source) != 1 or not beta['version'].endswith(f'.g{source[0][:12]}'):
        raise ValueError('Selected beta must be published with a matching source commit')
    source = source[0]
    subprocess.run(['git', 'merge-base', '--is-ancestor', source, sha], check=True)
    changed = git('diff', '--name-only', source, sha, '--', '.', ':(exclude)CHANGELOG.md')
    if changed:
        raise ValueError(f'Stable differs from the tested beta outside CHANGELOG.md; publish another beta:\n{changed}')
    pages = gh('api', '--paginate', '--slurp',
               f'repos/intuitums/e/deployments?sha={source}&environment=beta&per_page=100')
    for page in pages:
        for deployment in page:
            if deployment.get('payload', {}).get('tag') != beta_tag:
                continue
            statuses = gh('api', f'repos/intuitums/e/deployments/{deployment["id"]}/statuses')
            # GitHub marks an older successful deployment inactive after a newer one succeeds.
            if any(status['state'] == 'success' for status in statuses):
                return beta_tag
    raise ValueError('Selected beta has no successful verified deployment; finish or retry its release first')


if __name__ == '__main__':
    print(f'Promotion verified: {verify(sys.argv[1], sys.argv[2])}')
