#!/usr/bin/env python3
"""Compile an external SDK consumer from packed crates before publication.

The core need not exist on crates.io yet. Patch its exact pinned version into
the consumer from its packed crate, never from the working tree.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def stage(packages, target):
    """Pack the crates as Cargo would publish them and unpack each into `target`.

    A packed crate carries Cargo's normalized manifest: workspace fields are
    resolved and path dependencies are gone, exactly what crates.io serves.
    """
    args = ['cargo', 'package', '--allow-dirty', '--no-verify', '--locked']
    for package in packages:
        args += ['-p', package]
    subprocess.run(args, cwd=ROOT, check=True)
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT, text=True))
    versions = {p['name']: p['version'] for p in metadata['packages']}
    for package in packages:
        crate = Path(metadata['target_directory']) / 'package' / f'{package}-{versions[package]}.crate'
        with tarfile.open(crate) as archive:
            archive.extractall(target, filter='data')
        (target / f'{package}-{versions[package]}').rename(target / package)


def main():
    """Check the SDK's real example as a downstream binary, with isolated resolution."""
    manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text())
    app = manifest['workspace']['package']['version']
    sdk = tomllib.loads((ROOT / 'crates/sdk/Cargo.toml').read_text())
    assert sdk['dependencies']['ulo-core']['version'] == f'={app}', 'SDK must pin the current core version'
    with tempfile.TemporaryDirectory(prefix='ulo-sdk-consumer-') as tmp:
        root = Path(tmp)
        stage(['ulo-core', 'ulo-sdk'], root)
        consumer = root / 'consumer'
        (consumer / 'src').mkdir(parents=True)
        shutil.copyfile(root / 'ulo-sdk/examples/ask.rs', consumer / 'src/main.rs')
        (consumer / 'Cargo.toml').write_text('''[package]
name = "ulo-sdk-consumer"
version = "0.0.0"
edition = "2021"
[dependencies]
ulo_sdk = { package = "ulo-sdk", path = "../ulo-sdk" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
[patch.crates-io]
ulo-core = { path = "../ulo-core" }
''')
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT, text=True))
        subprocess.run(['cargo', 'check', '--manifest-path', str(consumer / 'Cargo.toml')], check=True,
                       env=dict(os.environ, CARGO_TARGET_DIR=metadata['target_directory']))


if __name__ == '__main__':
    main()
