#!/usr/bin/env python3
"""Compile an external SDK consumer from Cargo's package file lists before publication.

The application need not exist on crates.io yet. Patch its exact pinned version
into the consumer from a staged package, never from the working tree.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def stage(package, source, target):
    """Copy only distributable files and remove local workspace/path resolution."""
    names = subprocess.check_output(['cargo', 'package', '--list', '--allow-dirty', '-p', package],
                                    cwd=ROOT, text=True).splitlines()
    for name in names:
        path = source / name
        # Cargo generates these metadata files while packing; they are not source.
        if name in ('Cargo.toml.orig', '.cargo_vcs_info.json') or not path.is_file():
            continue
        dest = target / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, dest)
    manifest = target / 'Cargo.toml'
    text = re.sub(r'(?ms)^\[workspace\]\n.*?(?=^\[|\Z)', '', manifest.read_text())
    if package == 'intuitums-e-sdk':
        text = text.replace('path = "..", ', '')
    manifest.write_text(text)


def main():
    """Check the SDK's real example as a downstream binary, with isolated resolution."""
    app = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    sdk = tomllib.loads((ROOT / 'sdk/Cargo.toml').read_text())
    assert sdk['dependencies']['e']['version'] == f'={app}', 'SDK must pin the current application version'
    with tempfile.TemporaryDirectory(prefix='e-sdk-consumer-') as tmp:
        root = Path(tmp)
        stage('intuitums-e', ROOT, root / 'app')
        stage('intuitums-e-sdk', ROOT / 'sdk', root / 'sdk')
        consumer = root / 'consumer'
        (consumer / 'src').mkdir(parents=True)
        shutil.copyfile(root / 'sdk/examples/ask.rs', consumer / 'src/main.rs')
        (consumer / 'Cargo.toml').write_text('''[package]
name = "e-sdk-consumer"
version = "0.0.0"
edition = "2021"
[dependencies]
e_sdk = { package = "intuitums-e-sdk", path = "../sdk" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
[patch.crates-io]
intuitums-e = { path = "../app" }
''')
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT, text=True))
        subprocess.run(['cargo', 'check', '--manifest-path', str(consumer / 'Cargo.toml')], check=True,
                       env=dict(os.environ, CARGO_TARGET_DIR=metadata['target_directory']))


if __name__ == '__main__':
    main()
