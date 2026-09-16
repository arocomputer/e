#!/usr/bin/env python3
"""Install a pinned, verified dev artifact without waiting for npm publication."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from identity import identity

ROOT = Path(__file__).resolve().parents[2]


def gh(*args):
    """Read official repository metadata and artifacts using the contributor's gh login."""
    return subprocess.check_output(['gh', *args], text=True).strip()


def install(run_id):
    """Require a main release run and successful artifact verification, then use the installer."""
    if not re.fullmatch(r'[1-9][0-9]*', run_id):
        raise ValueError('Run ID must be a positive integer')
    run = json.loads(gh('api', f'repos/intuitums/e/actions/runs/{run_id}'))
    if (run['path'] != '.github/workflows/release.yml' or
            run['event'] != 'workflow_run' or run['head_branch'] != 'main'):
        raise ValueError('Choose a dev Release run from main')
    pages = json.loads(gh('api', '--paginate', '--slurp',
                         f'repos/intuitums/e/actions/runs/{run_id}/jobs?per_page=100'))
    if not any(job['name'] == 'Checksums and publish' and job['conclusion'] == 'success'
               for page in pages for job in page['jobs']):
        raise ValueError('Verified artifacts are not ready for this run')
    with tempfile.TemporaryDirectory() as tmp:
        gh('run', 'download', run_id, '--repo', 'intuitums/e', '--name', 'verified-assets', '--dir', tmp)
        root = Path(tmp)
        info = json.loads((root / 'build.json').read_text())
        release = identity(info['version'])
        if (release['channel'] != 'dev' or not re.fullmatch(r'[a-f0-9]{40}', info['commit']) or
                not release['version'].endswith(f'.g{info["commit"][:12]}')):
            raise ValueError('Artifact identity is not a dev build with a matching source commit')
        subprocess.run(['sh', str(ROOT / 'install.sh'), '--channel', 'dev', '--version', release['version']],
                       env=dict(os.environ, E_RELEASE_BASE=root.as_uri()), check=True)
        print(f'Installed source {info["commit"]} from run {run_id}. Repeat with a newer run to update.')


def main():
    """Expose an explicit run selection so failed npm publication cannot select another build."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run', help='Release run ID whose verified-assets upload has completed')
    args = parser.parse_args()
    install(args.run)


if __name__ == '__main__':
    main()
