"""Inspect settings; --apply enables requested safeguards without replacing policy."""
import argparse
import json
import copy
from pathlib import Path
import subprocess
import sys
from urllib.parse import quote
from github import pages, request

ACTION_APP = 15368
ROOT = Path(__file__).resolve().parents[2]


def required_checks(current):
    checks = copy.deepcopy(current.get('checks') or [])
    if not checks:
        checks = [{'context': context} for context in current.get('contexts', [])]
    if not any(check['context'] == 'ready' for check in checks):
        checks.append({'context': 'ready', 'app_id': ACTION_APP})
    for check in checks:
        if check['context'] == 'ready':
            check['app_id'] = ACTION_APP
    return {'strict': True, 'checks': checks}


def tag_rules():
    common = {'target': 'tag', 'enforcement': 'active', 'conditions': {
        'ref_name': {'include': ['refs/tags/v*'], 'exclude': []}}}
    return [
        {**common, 'name': 'release-tag-creation', 'bypass_actors': [
            {'actor_id': 5, 'actor_type': 'RepositoryRole', 'bypass_mode': 'always'}],
         'rules': [{'type': 'creation'}]},
        {**common, 'name': 'release-tags-immutable', 'bypass_actors': [],
         'rules': [{'type': 'update'}, {'type': 'deletion'}]},
    ]


def verify_main(repository, branch):
    head = request(f'repos/{repository}/branches/{quote(branch, safe="")}')['commit']['sha']
    checks = [check for page in pages(f'repos/{repository}/commits/{head}/check-runs?per_page=100')
              for check in page['check_runs'] if check['name'] == 'ready' and check['app']['id'] == ACTION_APP]
    if not checks or max(checks, key=lambda check: check['id'])['conclusion'] != 'success':
        raise ValueError('ready must pass on current main before changing protections')


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
    checks = required_checks(copy.deepcopy(current))
    rules = request(f'repos/{repository}/rulesets') or []
    pending_tags = []
    for desired in tag_rules():
        existing = [rule for rule in rules if rule['name'] == desired['name'] and rule['source'] == repository]
        if len(existing) > 1:
            raise ValueError('ambiguous release-tag rulesets')
        stored = request(f"repos/{repository}/rulesets/{existing[0]['id']}") if existing else None
        if stored is None or any(stored.get(key) != value for key, value in desired.items()):
            pending_tags.append((stored, desired))
    has_ready = any(item['context'] == 'ready' for item in current.get('checks', [])) or 'ready' in current.get('contexts', [])
    checks_missing = not current.get('strict') or not has_ready or checks['checks'] != current.get('checks', [])
    changes = []
    if not repo.get('delete_branch_on_merge'):
        changes.append('Enable automatic deletion of merged branches')
    if missing:
        changes.append('Enable or verify secret scanning and push protection (admin-visible settings required)')
    if checks_missing:
        changes.append(f'Require ready and an up-to-date branch on {branch}')
    if pending_tags:
        changes.append('Protect version-tag creation and forbid tag movement or deletion')
    print(json.dumps({'repository': repository, 'changes': changes}, indent=2))
    if not apply:
        return int(bool(changes))
    if changes:
        verify_main(repository, branch)
        backup = ROOT / 'target/settings'
        backup.mkdir(parents=True, exist_ok=True)
        (backup / 'before.json').write_text(json.dumps({'protection': protection, 'rulesets': rules,
                                                      'tag_rules': [item[0] for item in pending_tags],
                                                      'security': security}, indent=2) + '\n')
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
            request(endpoint, 'PUT', {'required_status_checks': checks, 'enforce_admins': True,
                                     'required_pull_request_reviews': None, 'restrictions': None})
    for stored, desired in pending_tags:
        if stored:
            request(f"repos/{repository}/rulesets/{stored['id']}", 'PUT', desired)
        else:
            request(f'repos/{repository}/rulesets', 'POST', desired)
    print('Requested settings applied; run the check again to verify.')
    return 0


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', default='arocomputer/e')
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    try:
        sys.exit(configure(args.repository, args.apply))
    except (subprocess.CalledProcessError, ValueError):
        sys.exit('GitHub admin access unavailable or the requested setting is unsupported. Check gh authentication and permissions; no further changes were attempted.')
