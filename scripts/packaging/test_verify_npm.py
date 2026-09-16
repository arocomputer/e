"""Exercise the real smoke-install retry with delayed metadata and platform binaries."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class RegistryInstallTests(unittest.TestCase):
    def run_verify(self, failure, ready):
        """Stub npm and sleep while preserving the installer's shell and clean prefixes."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            npm = root / 'npm'
            npm.write_text('''#!/bin/sh
count=0
[ ! -f "$STATE" ] || count=$(cat "$STATE")
count=$((count+1))
echo "$count" > "$STATE"
prefix=$3
[ ! -e "$prefix" ] || exit 99
mkdir -p "$prefix/node_modules/.bin"
if [ "$count" -lt "$READY" ]; then
  [ "$FAILURE" != metadata ] || exit 1
  [ "$FAILURE" != missing ] || exit 0
  echo '#!/bin/sh' > "$prefix/node_modules/.bin/e-dev"
  echo 'echo e wrong-version' >> "$prefix/node_modules/.bin/e-dev"
else
  echo '#!/bin/sh' > "$prefix/node_modules/.bin/e-dev"
  echo 'echo e "$VERSION"' >> "$prefix/node_modules/.bin/e-dev"
fi
chmod +x "$prefix/node_modules/.bin/e-dev"
''')
            npm.chmod(0o755)
            (root / 'sleep').write_text('#!/bin/sh\nexit 0\n')
            (root / 'sleep').chmod(0o755)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', STATE=str(root / 'calls'),
                       VERSION='1.2.3-dev.1.gabcdef012345', COMMAND='e-dev', FAILURE=failure, READY=str(ready))
            result = subprocess.run(['sh', str(ROOT / 'scripts/packaging/verify-npm.sh')],
                                    env=env, capture_output=True, text=True)
            return result.returncode, int((root / 'calls').read_text())

    def test_retries_until_the_installed_binary_works(self):
        for failure in ['metadata', 'missing', 'wrong-version']:
            with self.subTest(failure=failure):
                self.assertEqual(self.run_verify(failure, 3), (0, 3))

    def test_stops_on_first_success(self):
        self.assertEqual(self.run_verify('missing', 1), (0, 1))

    def test_fails_after_six_incomplete_installs(self):
        self.assertEqual(self.run_verify('missing', 7), (1, 6))
