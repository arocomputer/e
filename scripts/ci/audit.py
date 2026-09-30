"""Audit the active native/fuzz lockfiles and the pinned PTY requirements."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('ecosystem', choices=('rust', 'python'))
    args = parser.parse_args()
    if args.ecosystem == 'rust':
        subprocess.run(['cargo', 'install', 'cargo-audit', '--version', '0.22.2', '--locked'], cwd=ROOT, check=True)
        for lock in ('Cargo.lock', 'fuzz/Cargo.lock'):
            subprocess.run(['cargo', 'audit', '--file', lock], cwd=ROOT, check=True)
    else:
        environment = ROOT / 'target/audit-python'
        subprocess.run([sys.executable, '-m', 'venv', str(environment)], check=True)
        python = environment / ('Scripts/python.exe' if os.name == 'nt' else 'bin/python')
        subprocess.run([str(python), '-m', 'pip', 'install', 'pip-audit==2.10.1'], check=True)
        subprocess.run([str(python), '-m', 'pip_audit', '--disable-pip', '--no-deps', '--progress-spinner', 'off',
                        '-r', str(ROOT / 'crates/cli/tests/ui/requirements.txt')], check=True)


if __name__ == '__main__':
    main()
