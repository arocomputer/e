"""Inspect settings; --apply enables requested safeguards without replacing policy."""
import argparse
import json
import subprocess
import sys
from urllib.parse import quote
from github import request


def required_checks(current):
    checks = list(current.get('checks') or [])
    if not checks:
        checks = [{'context': context} for context in current.get('contexts', [])]
    if not any(check['context'] == 'ready' for check in checks):
        checks.append({'context': 'ready'})
    return {'strict': True, 'checks': checks}


def configure(repository, apply=False):
    repo = request(f'repos/{repository}')
    branch = repo['default_branch']
    endpoint = f'repos/{repository}/branches/{quote(branch, safe="")}/protection'
    try:
        protection = request(endpoint)
    except subprocess.CalledProcessError as error:
        if 'HTTP 404' not in (error.stderr or ''):
            raise
        protection = None
    security = repo.get('security_and_analysis', {})
    missing = [name for name in ('secret_scanning', 'secret_scanning_push_protection')
               if security.get(name, {}).get('status') != 'enabled']
    current = (protection or {}).get('required_status_checks') or {}
    checks = required_checks(current)
    has_ready = any(item['context'] == 'ready' for item in current.get('checks', [])) or 'ready' in current.get('contexts', [])
    checks_missing = not current.get('strict') or not has_ready
    changes = []
    if not repo.get('delete_branch_on_merge'):
        changes.append('Enable automatic deletion of merged branches')
    if missing:
        changes.append('Enable or verify secret scanning and push protection (admin-visible settings required)')
    if checks_missing:
        changes.append(f'Require ready and an up-to-date branch on {branch}')
    print(json.dumps({'repository': repository, 'changes': changes}, indent=2))
    if not apply:
        return int(bool(changes))
    body = {}
    if not repo.get('delete_branch_on_merge'):
        body['delete_branch_on_merge'] = True
    if missing:
        body['security_and_analysis'] = {name: {'status': 'enabled'} for name in missing}
    if body:
        request(f'repos/{repository}', 'PATCH', body)
    if checks_missing:
        if protection:
            # Update only checks: retain existing reviews, bypass rules, and restrictions.
            request(endpoint + '/required_status_checks', 'PATCH', checks)
        else:
            request(endpoint, 'PUT', {'required_status_checks': checks, 'enforce_admins': False,
                                     'required_pull_request_reviews': None, 'restrictions': None})
    print('Requested settings applied; run the check again to verify.')
    return 0


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', default='arocomputer/e')
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    try:
        sys.exit(configure(args.repository, args.apply))
    except subprocess.CalledProcessError:
        sys.exit('GitHub admin access unavailable or the requested setting is unsupported. Check gh authentication and permissions; no further changes were attempted.')
