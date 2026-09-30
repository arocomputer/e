"""Check external documentation links outside the PR gate."""
import subprocess
from tools import ROOT, ensure


def main():
    files = sorted(ROOT.glob('*.md')) + sorted((ROOT / 'docs').rglob('*.md'))
    report = ROOT / 'target/links.md'
    report.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([ensure('lychee'), '--config', str(ROOT / '.lychee.toml'),
                    '--format', 'markdown', '--output', str(report),
                    *map(str, files)], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
