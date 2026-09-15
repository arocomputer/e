"""Pin artifact verification and the package-manager ownership contract."""

import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from prepare import prepare, PLATFORMS


class Packages(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.assets = self.root / "assets"
        self.assets.mkdir()
        checksums = []
        for target in PLATFORMS.values():
            path = self.assets / f"e-{target}.tar.gz"
            data = b'#!/bin/sh\nprintf "%s\\n" "$@"\n'
            with tarfile.open(path, "w:gz") as archive:
                member = tarfile.TarInfo("e")
                member.size = len(data)
                member.mode = 0o755
                archive.addfile(member, io.BytesIO(data))
            checksums.append(
                f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}"
            )
        (self.assets / "checksums.txt").write_text("\n".join(checksums))

    def tearDown(self):
        self.temp.cleanup()

    def test_one_version_and_owned_binaries_without_install_scripts(self):
        output = self.root / "dist"
        prepare("v1.2.3", self.assets, output)
        wrapper = json.loads((output / "e/package.json").read_text())
        self.assertEqual(set(wrapper["optionalDependencies"].values()), {"1.2.3"})
        self.assertNotIn("scripts", wrapper)
        for platform in PLATFORMS:
            package = json.loads((output / platform / "package.json").read_text())
            self.assertEqual(package["version"], "1.2.3")
            self.assertEqual(
                (output / platform / "bin/.e-install-method").read_text(), "npm\n"
            )
        formula = (output / "e.rb").read_text()
        self.assertIn('version "1.2.3"', formula)
        self.assertEqual(formula.count("sha256 "), 4)
        self.assertIn('libexec/".e-install-method"', formula)

    def test_corrupt_archive_fails_before_creating_packages(self):
        (self.assets / f"e-{next(iter(PLATFORMS.values()))}.tar.gz").write_bytes(
            b"corrupt"
        )
        output = self.root / "dist"
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            prepare("v1.2.3", self.assets, output)
        self.assertFalse(output.exists())

    def test_legacy_release_cannot_overwrite_managed_installations(self):
        with self.assertRaisesRegex(ValueError, "predates"):
            prepare("v0.0.1", self.assets, self.root / "dist")

    def test_unknown_prerelease_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Expected"):
            prepare("v1.2.3-rc.1", self.assets, self.root / "dist")

    def test_preview_packages_keep_the_channel_and_separate_command(self):
        output = self.root / "dist"
        prepare("v1.2.3-beta.12.gabcdef012345", self.assets, output)
        wrapper = json.loads((output / "e/package.json").read_text())
        self.assertEqual(wrapper["publishConfig"]["tag"], "beta")
        self.assertEqual(wrapper["bin"], {"e-beta": "bin/e"})
        self.assertEqual((output / "darwin-arm64/bin/.e-install-method").read_text(), "npm-beta\n")
        formula = (output / "e-beta.rb").read_text()
        self.assertIn('class EBeta < Formula', formula)
        self.assertIn('=> "e-beta"', formula)


if __name__ == "__main__":
    unittest.main()
