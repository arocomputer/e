"""The final guard rule must fail the process, just like earlier rules."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class GuardTests(unittest.TestCase):
    def test_direct_check_command_exits_unsuccessfully(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'scripts').mkdir()
            shutil.copyfile(Path(__file__).resolve().parents[1] / 'guard.sh', root / 'scripts/guard.sh')
            for crate in ('core', 'tui', 'rpc', 'sdk', 'cli'):
                folder = root / 'crates' / crate
                (folder / 'src').mkdir(parents=True)
                (folder / 'src/lib.rs').write_text('')
                (folder / 'Cargo.toml').write_text('')
            (root / 'crates/cli/tests/ui').mkdir(parents=True)
            (root / '.github/workflows').mkdir(parents=True)
            (root / '.github/CODEOWNERS').write_text('* @maintainer\n')
            workflow = root / '.github/workflows/test.yml'
            for command, status in [('./x test', 0), ('cargo test', 1)]:
                with self.subTest(command=command):
                    workflow.write_text(f'run: {command}\n')
                    result = subprocess.run(['sh', 'scripts/guard.sh'], cwd=root,
                                            text=True, capture_output=True)
                    self.assertEqual(result.returncode, status, result.stdout + result.stderr)
                    if status:
                        self.assertIn('a check calls a tool directly', result.stdout)
                        self.assertNotIn('all checks passed', result.stdout)
