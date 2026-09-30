"""Delete only a closed PR's merge-ref caches, never shared branch caches."""
import json
import os
from pathlib import Path
from urllib.parse import quote
from github import pages, request


def cleanup(repository, event):
    pr = event['pull_request']
    number = event['number']
    if event.get('action') != 'closed' or pr.get('state') != 'closed':
        raise ValueError('cache cleanup requires a closed pull request')
    if not isinstance(number, int) or isinstance(number, bool) or number < 1:
        raise ValueError('invalid pull request number')
    ref = f'refs/pull/{number}/merge'
    # Snapshot all pages before deleting; deleting while paging shifts results.
    caches = [cache for page in pages(f'repos/{repository}/actions/caches?ref={quote(ref, safe="")}&per_page=100')
              for cache in page['actions_caches']]
    count = 0
    for cache in caches:
        if cache['ref'] != ref:
            continue
        cache_id = cache['id']
        if not isinstance(cache_id, int) or isinstance(cache_id, bool) or cache_id < 1:
            raise ValueError('invalid cache id')
        request(f'repos/{repository}/actions/caches/{cache_id}', 'DELETE')
        count += 1
    print(f'Deleted {count} closed-PR caches for #{number}')


if __name__ == '__main__':
    cleanup(os.environ['GITHUB_REPOSITORY'], json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text()))
