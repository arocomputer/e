#!/usr/bin/env python3
"""Select CI jobs from changed paths; unknown paths run every job."""
import json
import os
import re
from pathlib import Path
import subprocess


GATES = ('build', 'packages', 'channels', 'docs', 'lock', 'bench', 'ui', 'crates', 'glibc', 'site', 'python', 'fuzz')
JOBS = ('lint', 'test-linux', 'test-macos', 'ui', 'docs', 'site', 'packages-linux',
        'packages-macos', 'crates', 'channels', 'glibc', 'audit', 'bench', 'fuzz')


def classify(paths):
    """Select runtime consumers separately from repository maintenance tooling."""
    gates = dict.fromkeys(GATES, False)
    for path in paths:
        if path.startswith('docs/guides/') or path in ('docs/README.md', 'scripts/ci/site.py', 'scripts/ci/render_guides.py'):
            gates['site'] = True
        if path == 'scripts/ci/requirements-site.txt':
            gates['site'] = gates['python'] = True
            continue
        if path.endswith('.md') or path.startswith('docs/'):
            gates['docs'] = True
            continue
        if path == 'LICENSE':
            gates['packages'] = gates['crates'] = True
            continue
        if path.startswith(('assets/', 'benchmarks/results/')) or path in (
                'LICENSE', '.gitignore', '.gitattributes', '.editorconfig',
                '.github/CODEOWNERS', '.github/dependabot.yml', '.github/infra-tools.json', '.lychee.toml'):
            continue
        if path.startswith(('services/slack/', 'services/github/', 'crates/cli/tests/fixtures/channels/')):
            gates['channels'] = True
            continue
        if path == 'crates/cli/tests/ui/requirements.txt':
            gates['python'] = gates['ui'] = True
            continue
        if path.startswith('fuzz/'):
            gates['fuzz'] = True
            gates['lock'] |= path.endswith(('Cargo.toml', 'Cargo.lock'))
            continue
        if path == 'scripts/ci/audit.py':
            gates['lock'] = gates['python'] = True
            continue
        if path.startswith('scripts/ci/') and Path(path).name not in ('changes.py', 'ready.py', 'test_changes.py', 'test_ready.py'):
            continue  # lint always executes these tools' tests and workflow scanners.
        if path.startswith('.github/workflows/'):
            if Path(path).name not in ('ci.yml', 'release.yml', 'preview.yml', 'registry.yml'):
                continue
            gates.update(dict.fromkeys(GATES, True))
            continue
        if path.startswith(('scripts/packaging/', 'scripts/release/')) or path in (
                'install.sh', 'scripts/release-check.sh', 'scripts/release-notes.sh'):
            gates['packages'] = gates['glibc'] = True
            continue
        if path.startswith('crates/') and (path.endswith('.rs') or '/themes/' in path or '/tests/' in path):
            gates['build'] = True
            gates['fuzz'] |= path.startswith(('crates/core/src/', 'crates/tui/src/'))
            gates['crates'] |= '/src/' in path or '/themes/' in path or path.endswith('/build.rs')
            gates['bench'] |= path.startswith(('crates/tui/src/', 'crates/core/src/', 'crates/cli/src/')) or path.endswith('/build.rs') or '/themes/' in path
            gates['ui'] |= path.startswith(('crates/tui/', 'crates/core/src/', 'crates/cli/src/', 'crates/cli/tests/ui/', 'crates/cli/tests/common/')) or path == 'crates/cli/tests/parity.rs'
            if path.endswith('/build.rs') or path.endswith('/src/update.rs') or path == 'crates/cli/tests/update.rs':
                gates['packages'] = gates['glibc'] = True
            continue
        if path in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml') or (path.startswith('crates/') and path.endswith('/Cargo.toml')):
            gates['build'] = gates['crates'] = gates['ui'] = gates['lock'] = gates['glibc'] = gates['bench'] = gates['fuzz'] = True
            continue
        if path.startswith('benchmarks/'):
            gates['bench'] = True
            continue
        gates.update(dict.fromkeys(GATES, True))  # Unknown inputs are conservative.
    return gates


def job_plan(gates, event):
    """Use one selection contract for workflow conditions and the final gate."""
    return {
        'fuzz': gates['fuzz'], 'lint': True, 'test-linux': gates['build'], 'test-macos': gates['build'],
        'ui': gates['ui'], 'docs': gates['docs'] and not gates['build'], 'site': gates['site'],
        'packages-linux': gates['packages'], 'packages-macos': gates['packages'],
        'crates': gates['crates'], 'channels': gates['channels'] or event == 'schedule',
        'glibc': gates['glibc'], 'audit': gates['lock'] or gates['python'] or event == 'schedule',
        'bench': gates['bench'] or event in ('schedule', 'workflow_dispatch') or (event == 'push' and gates['build']),
    }


def changed_paths():
    """Read both sides of PR renames, or the main commit's changes."""
    if os.environ.get('PR'):
        raw = subprocess.check_output([
            'gh', 'api', f'repos/{os.environ["GITHUB_REPOSITORY"]}/pulls/{os.environ["PR"]}/files',
            '--paginate', '--slurp'], text=True)
        pages = json.loads(raw)
        if sum(len(page) for page in pages) >= 3000:
            raise ValueError('PR file listing may be truncated')
        return [name for page in pages for item in page
                for name in (item['filename'], item.get('previous_filename')) if name]
    event_path = os.environ.get('GITHUB_EVENT_PATH')
    before = json.loads(Path(event_path).read_text()).get('before') if event_path else None
    if before is not None and not re.fullmatch('[0-9a-f]{40}', before):
        raise ValueError('invalid push base')
    return subprocess.check_output(
        ['git', 'diff', '--name-only', before or 'HEAD^', 'HEAD'], text=True).splitlines()


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
    selected = job_plan(gates, event)
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with Path(os.environ['GITHUB_STEP_SUMMARY']).open('a') as summary:
            summary.write(f"Selected {sum(selected.values())} of {len(selected)} check jobs.\n\n| Check | Selection |\n| --- | --- |\n")
            for job, enabled in selected.items():
                summary.write(f"| {job} | {'run' if enabled else 'skip'} |\n")
    with Path(os.environ['GITHUB_OUTPUT']).open('a') as output:
        output.write('plan=' + json.dumps(selected, separators=(',', ':')) + '\n')
        for name, enabled in gates.items():
            output.write(f'{name}={str(enabled).lower()}\n')


if __name__ == '__main__':
    main()
