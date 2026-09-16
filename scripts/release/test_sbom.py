"""The release must select the application's BOM after the crates move."""
from pathlib import Path
import unittest

from sbom import application_bom


class SbomTests(unittest.TestCase):
    def test_workspace_selects_cli_instead_of_root_or_sdk(self):
        metadata = {'packages': [
            {'name': 'intuitums-e-core', 'manifest_path': '/repo/crates/core/Cargo.toml'},
            {'name': 'intuitums-e-sdk', 'manifest_path': '/repo/crates/sdk/Cargo.toml'},
            {'name': 'intuitums-e', 'manifest_path': '/repo/crates/cli/Cargo.toml'},
        ]}
        self.assertEqual(application_bom(metadata), Path('/repo/crates/cli/e-release-sbom.json'))

    def test_root_package_remains_publishable(self):
        metadata = {'packages': [{'name': 'intuitums-e', 'manifest_path': '/repo/Cargo.toml'}]}
        self.assertEqual(application_bom(metadata), Path('/repo/e-release-sbom.json'))

    def test_no_application_fails_instead_of_shipping_another_crate(self):
        with self.assertRaisesRegex(ValueError, 'application'):
            application_bom({'packages': []})


if __name__ == '__main__':
    unittest.main()
