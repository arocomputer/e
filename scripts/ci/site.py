"""Render guides without private repository access; optionally build a local website."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def build(source):
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    env = dict(os.environ, E_DOCS_PATH=str(ROOT), E_DOCS_REF=commit)
    for command in (['npm', 'ci'], ['npm', 'test'], ['npm', 'run', 'typecheck'], ['npm', 'run', 'build']):
        subprocess.run(command, cwd=source, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, help='Local website checkout for development')
    args = parser.parse_args()
    if args.source:
        build(args.source.resolve())
        return
    environment = ROOT / 'target/site-python'
    subprocess.run([sys.executable, '-m', 'venv', str(environment)], check=True)
    python = environment / ('Scripts/python.exe' if os.name == 'nt' else 'bin/python')
    subprocess.run([str(python), '-m', 'pip', 'install', '--disable-pip-version-check',
                    '-r', str(ROOT / 'scripts/ci/requirements-site.txt')], check=True)
    subprocess.run([str(python), str(ROOT / 'scripts/ci/render_guides.py')], check=True)



if __name__ == '__main__':
    main()
