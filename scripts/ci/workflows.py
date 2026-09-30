"""The same workflow checks run locally and in the required CI gate."""
import subprocess
from tools import ROOT, ensure


def main():
    # GitHub supports queue:max, but actionlint 1.7.12 does not yet know this key.
    # Ignore only this diagnostic; retain all other concurrency/schema checks.
    subprocess.run([ensure('actionlint'), '-shellcheck=', '-pyflakes=', '-ignore',
                    'unexpected key "queue" for "concurrency" section'], cwd=ROOT, check=True)
    subprocess.run([ensure('zizmor'), '--offline', '--no-progress', '--min-severity', 'medium',
                    '--strict-collection', '.github/workflows'], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
