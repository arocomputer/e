#!/usr/bin/env python3
"""Keep one dev deployment comment on the PR that produced the selected main commit."""
import json
import os
import re
import subprocess

MARKER = '<!-- e-dev-deployment -->'


def api(repository, path, data=None, method=None):
    """Read paginated GitHub data or write JSON without shell interpolation."""
    args = ['gh', 'api', f'repos/{repository}/{path}']
    if data is None:
        pages = json.loads(subprocess.check_output(args + ['--paginate', '--slurp'], text=True))
        return [item for page in pages for item in page]
    return json.loads(subprocess.check_output(
        args + ['--method', method or 'POST', '--input', '-'],
        input=json.dumps(data), text=True))


def render(needs, stage, run_url):
    """Describe the first failed stage and offer commands only after verification succeeds."""
    release = needs['resolve']['outputs']
    status = {'building': 'Building', 'publishing': 'Publishing'}.get(stage, 'Ready')
    detail = 'Compiling and checking the macOS and Linux builds.' if stage == 'building' else 'Publishing packages and verifying installation.'
    if stage == 'finished':
        detail = 'macOS and Linux packages published; npm installation verified on Linux x64.'
        labels = {
            'qualify': 'Source checks', 'create': 'Release preparation', 'build': 'Native build',
            'publish': 'Artifact verification', 'npm': 'npm publication',
        }
        for job, label in labels.items():
            result = needs.get(job, {}).get('result')
            if result in ('failure', 'cancelled'):
                status = 'Cancelled' if result == 'cancelled' else 'Failed'
                detail = f'{label} {result}. Open the workflow logs for the failed step.'
                if job == 'npm':
                    outputs = needs[job].get('outputs', {})
                    if outputs.get('verify') == 'failure':
                        detail = 'Installation verification failed. Inspect the install or executable error before using this build.'
                    elif outputs.get('cleanup') == 'failure':
                        detail = 'Preview tag cleanup failed. Check npm publishing permissions, then rerun failed jobs.'
                    elif outputs.get('failure') == 'processing':
                        detail = 'npm processing timed out. Check the package status on npm, then rerun failed jobs to resume verification.'
                    elif outputs.get('failure') == 'authorization':
                        detail = 'npm authorization failed. Check trusted publishing or the publishing token permissions, then rerun failed jobs.'
                    elif outputs.get('failure') == 'integrity':
                        detail = 'Published package integrity differs from this build. Investigate the existing version and retained artifacts before retrying.'
                    else:
                        detail = ('npm publication did not complete. If npm is still processing a package, rerun failed jobs to resume verification. '
                                  'For an authorization error, check the npm publishing credentials first.')
                elif result == 'failure':
                    detail += ' Fix the reported error and push a new commit; rerun failed jobs for a transient runner failure.'
                break
        else:
            if needs.get('npm', {}).get('result') != 'success':
                status, detail = 'Incomplete', 'Publication did not run. Check the workflow for skipped or cancelled prerequisites.'
    body = f'### Dev deployment: {status}\n\n`{release["version"]}` · Commit `{release["sha"][:12]}`\n\n{detail}\n'
    if stage == 'finished' and status == 'Ready':
        body += f'\nInstall this exact build:\n\n```sh\nnpm install -g @intuitums/e@{release["version"]}\ne-dev\n```\n'
        body += f'\nWith bun: `bun add -g @intuitums/e@{release["version"]}`\n'
    return body + f'\n[View workflow and logs]({run_url})\n'


def report(needs, stage, repository, server, run_id, attempt):
    """Update only the matching merged PR and never overwrite a newer run or stage."""
    release = needs['resolve']['outputs']
    if release.get('channel') != 'dev' or release.get('mode') == 'skip':
        return
    run_url = f'{server}/{repository}/actions/runs/{run_id}/attempts/{attempt}'
    body = render(needs, stage, run_url)
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as summary:
            summary.write(body)
    order = (int(run_id), int(attempt), {'building': 0, 'publishing': 1, 'finished': 2}[stage])
    body = f'{MARKER}\n<!-- run:{order[0]} attempt:{order[1]} stage:{order[2]} -->\n{body}'
    for pr in api(repository, f'commits/{release["sha"]}/pulls'):
        if not (pr.get('merged_at') and pr.get('merge_commit_sha') == release['sha']
                and pr['base']['ref'] == 'main' and pr['base']['repo']['full_name'] == repository):
            continue
        comments = api(repository, f'issues/{pr["number"]}/comments')
        previous = next((c for c in comments if c['user']['login'] == 'github-actions[bot]'
                         and c['body'].startswith(MARKER)), None)
        if previous:
            match = re.search(r'<!-- run:(\d+) attempt:(\d+) stage:(\d+) -->', previous['body'])
            if match and tuple(map(int, match.groups())) > order:
                continue
            api(repository, f'issues/comments/{previous["id"]}', {'body': body}, 'PATCH')
        else:
            api(repository, f'issues/{pr["number"]}/comments', {'body': body})


if __name__ == '__main__':
    report(json.loads(os.environ['RELEASE_JOBS']), os.environ['REPORT_STAGE'],
           os.environ['GITHUB_REPOSITORY'], os.environ['GITHUB_SERVER_URL'],
           os.environ['GITHUB_RUN_ID'], os.environ['GITHUB_RUN_ATTEMPT'])
