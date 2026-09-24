#!/usr/bin/env python3
"""Select checks and dev publication from changed paths; unknown paths run checks."""
import json
import os
from pathlib import Path
import subprocess


def classify(paths):
    """Return independent gates; artwork/prose cannot trigger a native build or release."""
    gates = dict.fromkeys(('build', 'packages', 'channels', 'docs', 'lock', 'bench', 'publish'), False)
    for path in paths:
        # The website workflow checks and deploys this independent frontend.
        if path.startswith('services/www/') or path == '.github/workflows/www.yml':
            continue
        workflow = path.startswith('.github/workflows/')
        if path.startswith(('services/slack/', 'services/github/', 'crates/cli/tests/fixtures/channels/')) or workflow:
            gates['channels'] = True
        if (path.startswith(('scripts/packaging/', 'scripts/release/', 'scripts/ci/')) or workflow
                or path in ('crates/core/build.rs', 'install.sh', 'LICENSE',
                            'crates/core/src/update.rs', 'crates/cli/src/update.rs', 'crates/cli/tests/update.rs',
                            'scripts/release-check.sh', 'x')):
            gates['packages'] = True
        if path in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml') or (
                path.startswith('crates/') and path.endswith('/Cargo.toml')):
            gates['lock'] = True
        if path.startswith('docs/') or path.endswith('.md'):
            gates['docs'] = True
        if (path.startswith(('crates/tui/', 'crates/core/', 'crates/cli/src/', 'benchmarks/'))
                and not path.endswith('.md') and not path.startswith('benchmarks/results/')) or (
                path in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'x') or workflow):
            gates['bench'] = True
        # These files do not change Rust compilation. Guides get their own
        # contract and site build; the release carries them with the next code change.
        if (path.endswith('.md') or path.startswith(('docs/', 'assets/', 'services/slack/', 'services/github/', 'benchmarks/results/'))
                or path in ('LICENSE', '.gitignore', '.gitattributes', '.editorconfig',
                            '.github/CODEOWNERS', '.github/dependabot.yml')):
            continue
        gates['build'] = gates['publish'] = True
    gates['bench'] |= gates['lock']
    return gates


def changed_paths():
    """Read both sides of PR renames, or the main commit's changes."""
    if os.environ.get('PR'):
        raw = subprocess.check_output([
            'gh', 'api', f'repos/{os.environ["GITHUB_REPOSITORY"]}/pulls/{os.environ["PR"]}/files',
            '--paginate', '--slurp'], text=True)
        return [name for page in json.loads(raw) for item in page
                for name in (item['filename'], item.get('previous_filename')) if name]
    return subprocess.check_output(
        ['git', 'diff', '--name-only', 'HEAD^', 'HEAD'], text=True).splitlines()


def main():
    """Write Actions outputs, running every layer if the file listing fails."""
    try:
        event = os.environ.get('GITHUB_EVENT_NAME')
        if event in ('schedule', 'workflow_dispatch'):
            gates = dict.fromkeys(classify([]), event == 'workflow_dispatch')
        else:
            gates = classify(changed_paths())
    except (subprocess.CalledProcessError, ValueError, KeyError):
        print('Cannot list changed files; running every check.')
        gates = dict.fromkeys(classify([]), True)
    with Path(os.environ['GITHUB_OUTPUT']).open('a') as output:
        for name, enabled in gates.items():
            output.write(f'{name}={str(enabled).lower()}\n')


if __name__ == '__main__':
    main()
