#!/usr/bin/env python3
"""Require every CI workflow to pass on the exact source of a dev release."""
import json
import os
import re
import subprocess
import sys

WORKFLOWS = ('lint', 'unit', 'e2e', 'docs', 'packages', 'channels', 'glibc', 'bench')


def select(pages, sha):
    """Select the latest main push run per workflow; missing evidence fails closed."""
    runs = {}
    for page in pages:
        for run in page['workflow_runs']:
            if run['head_sha'] != sha or run['head_branch'] != 'main' or run['event'] != 'push':
                continue
            path = run['path']
            if path not in runs or run['id'] > runs[path]['id']:
                runs[path] = run
    required = [f'.github/workflows/{name}.yml' for name in WORKFLOWS]
    missing = [path for path in required if path not in runs]
    if missing:
        raise ValueError('Missing source checks: ' + ', '.join(missing))
    return [runs[path] for path in required]


def verify(sha):
    """Wait for running checks and reject failed, cancelled, or missing workflows."""
    if not re.fullmatch(r'[a-f0-9]{40}', sha):
        raise ValueError('Expected a full source commit')
    repo = os.environ['GITHUB_REPOSITORY']
    pages = json.loads(subprocess.check_output([
        'gh', 'api', '--paginate', '--slurp',
        f'repos/{repo}/actions/runs?head_sha={sha}&event=push&branch=main&per_page=100',
    ], text=True))
    for run in select(pages, sha):
        if run['status'] == 'completed':
            if run['conclusion'] != 'success':
                raise ValueError(f"Source check failed: {run['path']} ({run['conclusion']})")
        else:
            subprocess.run([
                'gh', 'run', 'watch', str(run['id']), '--repo', repo,
                '--exit-status', '--interval', '10',
            ], check=True)


if __name__ == '__main__':
    verify(sys.argv[1])
