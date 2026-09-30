"""Keep one bot-owned issue for each failing scheduled workflow."""
import json
import os
from pathlib import Path
from github import pages, request

WORKFLOWS = {'ci', 'fuzz', 'links'}


def report(repository, event):
    run = event['workflow_run']
    if (run['name'] not in WORKFLOWS or run['event'] != 'schedule'
            or run['head_branch'] != event['repository']['default_branch']
            or (run.get('head_repository') or {}).get('full_name') != repository
            or run['conclusion'] not in ('success', 'failure', 'timed_out')):
        return
    latest = request(f'repos/{repository}/actions/workflows/{run["workflow_id"]}/runs?event=schedule&per_page=1')['workflow_runs']
    if (not latest or latest[0]['id'] != run['id']
            or latest[0].get('run_attempt', 1) != run.get('run_attempt', 1)):
        return  # A delayed completion must not overwrite a newer run's result.
    marker = f'<!-- scheduled-workflow:{run["workflow_id"]} -->'
    issues = [issue for page in pages(f'repos/{repository}/issues?state=all&creator=github-actions%5Bbot%5D&per_page=100')
              for issue in page if not issue.get('pull_request') and issue['user']['login'] == 'github-actions[bot]'
              and marker in (issue.get('body') or '')]
    issue = max(issues, key=lambda item: item['number'], default=None)
    if run['conclusion'] == 'success':
        if issue and issue['state'] == 'open':
            request(f'repos/{repository}/issues/{issue["number"]}', 'PATCH', {'state': 'closed', 'state_reason': 'completed'})
        return
    body = (f'{marker}\nThe scheduled **{run["name"]}** workflow failed.\n\n'
            f'[Inspect run #{run["run_number"]}]({run["html_url"]}). '
            'This issue is updated on repeated failures and closed after a successful scheduled run.\n')
    payload = {'title': f'Scheduled {run["name"]} workflow is failing', 'body': body, 'state': 'open'}
    if issue:
        if issue.get('body') != body or issue['state'] != 'open':
            request(f'repos/{repository}/issues/{issue["number"]}', 'PATCH', payload)
    else:
        request(f'repos/{repository}/issues', 'POST', {'title': payload['title'], 'body': body})


if __name__ == '__main__':
    report(os.environ['GITHUB_REPOSITORY'], json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text()))
