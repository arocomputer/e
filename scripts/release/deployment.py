#!/usr/bin/env python3
"""Record the release outcome against its source commit in GitHub deployments."""
import json
import os
from pathlib import Path
import subprocess


def report(needs, repository, server, run_id):
    """Report completed release jobs; never substitute the workflow branch for the source."""
    release = needs['resolve']['outputs']
    channel = release['channel']
    assert channel in ('stable', 'beta', 'dev')
    environment = 'production' if channel == 'stable' else channel
    required = ('npm',) if channel == 'dev' else ('npm', 'homebrew', 'channel')
    success = all(needs[job]['result'] == 'success' for job in required)
    state = 'success' if success else 'error' if any(job['result'] == 'cancelled' for job in needs.values()) else 'failure'
    run_url = f'{server}/{repository}/actions/runs/{run_id}'
    release_url = (f'https://www.npmjs.com/package/@intuitums/e/v/{release["version"]}' if channel == 'dev'
                   else f'{server}/{release["repository"]}/releases/tag/{release["tag"]}')

    def post(path, data):
        """Send structured JSON through stdin so metadata never becomes shell code."""
        return json.loads(subprocess.check_output(
            ['gh', 'api', '--method', 'POST', f'repos/{repository}/{path}', '--input', '-'],
            input=json.dumps(data), text=True))

    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with Path(os.environ['GITHUB_STEP_SUMMARY']).open('a') as summary:
            summary.write('Release distribution results\n\n| Distribution | Result |\n| --- | --- |\n')
            for job, label in [('publish', 'Verified binaries'), ('channel', 'Website installer'),
                               ('npm', 'npm and bun'), ('homebrew', 'Homebrew'),
                               ('container', 'Container'), ('crates', 'crates.io')]:
                summary.write(f'| {label} | {needs.get(job, {}).get("result", "skipped")} |\n')

    deployment = post('deployments', {
        'ref': release['sha'], 'environment': environment,
        'auto_merge': False, 'required_contexts': [],
        'production_environment': channel == 'stable',
        'description': release['version'],
        'payload': {'tag': release['tag'], 'run_url': run_url},
    })
    post(f'deployments/{deployment["id"]}/statuses', {
        'state': state, 'log_url': run_url,
        'environment_url': release_url if success else run_url,
        'description': 'Packages and installers verified' if success else 'Release incomplete; see workflow logs',
        'auto_inactive': True,
    })


if __name__ == '__main__':
    report(json.loads(os.environ['RELEASE_JOBS']), os.environ['GITHUB_REPOSITORY'],
           os.environ['GITHUB_SERVER_URL'], os.environ['GITHUB_RUN_ID'])
