#!/usr/bin/env python3
"""Request or install an exact PR build using GitHub Actions and an isolated executable."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile


def gh(*args):
    return subprocess.check_output(['gh', *args], text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pr', type=int)
    parser.add_argument('--run', help='Install artifacts from this completed Preview workflow run')
    args = parser.parse_args()
    if args.pr <= 0:
        parser.error('PR must be positive')
    if not args.run:
        gh('workflow', 'run', 'preview.yml', '--repo', 'intuitums/e', '-f', f'pr={args.pr}')
        print('Requested preview. Find its run with: gh run list --repo intuitums/e --workflow preview.yml')
        print(f'After it succeeds: ./x preview {args.pr} --run RUN_ID')
        return
    if not args.run.isdigit():
        parser.error('run must be numeric')
    run = json.loads(gh('api', f'repos/intuitums/e/actions/runs/{args.run}'))
    assert run['path'] == '.github/workflows/preview.yml' and run['conclusion'] == 'success', 'Not a successful Preview run'
    os_name = {'Darwin': 'apple-darwin', 'Linux': 'unknown-linux-gnu'}[platform.system()]
    arch = {'arm64': 'aarch64', 'aarch64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()]
    with tempfile.TemporaryDirectory() as tmp:
        gh('run', 'download', args.run, '--repo', 'intuitums/e', '--name', f'e-pr-{args.pr}-{arch}-{os_name}', '--dir', tmp)
        root = Path(tmp)
        info = json.loads((root / 'preview.json').read_text())
        assert info['channel'] == 'pr'
        digest = hashlib.sha256((root / 'e').read_bytes()).hexdigest()
        assert (root / 'checksums.txt').read_text().split() == [digest, 'e'], 'Checksum mismatch'
        # Pin the requested artifact, even if the PR has received newer commits since the run.
        dest = Path(os.environ.get('E_INSTALL_DIR', Path.home() / '.local/bin'))
        dest.mkdir(parents=True, exist_ok=True)
        target = dest / f'e-pr-{args.pr}'
        if target.is_symlink() or (dest / '.e-install-method').exists():
            raise SystemExit('Destination is package-managed; set E_INSTALL_DIR elsewhere')
        staged = dest / f'.e-pr-{args.pr}.next'
        staged.write_bytes((root / 'e').read_bytes())
        staged.chmod(0o755)
        staged.replace(target)
        print(f'Installed {target} from {info["commit"]}. PR code is unreviewed; use a disposable project.')


if __name__ == '__main__':
    main()
