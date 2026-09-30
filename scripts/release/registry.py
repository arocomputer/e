"""Prepare Cargo's registry upload protocol using the exact verified crate archives."""
import hashlib
import json
from pathlib import Path
import struct
import sys
import tarfile
import tomllib


def payload(archive):
    """Serialize normalized package metadata without rebuilding or executing the crate."""
    with tarfile.open(archive) as crate:
        prefix = archive.name.removesuffix(".crate") + "/"
        manifest = tomllib.loads(crate.extractfile(prefix + "Cargo.toml").read().decode())
        package = manifest["package"]
        metadata = {"name": package["name"], "vers": package["version"],
                    "features": manifest.get("features", {}), "deps": [], "badges": manifest.get("badges", {})}
        for key in ("authors", "keywords", "categories"):
            metadata[key] = package.get(key, [])
        for key in ("description", "documentation", "homepage", "license", "repository", "links"):
            metadata[key] = package.get(key)
        metadata["license_file"] = package.get("license-file")
        metadata["rust_version"] = package.get("rust-version")
        readme = package.get("readme")
        metadata["readme_file"] = readme if isinstance(readme, str) else None
        metadata["readme"] = crate.extractfile(prefix + readme).read().decode() if isinstance(readme, str) else None
        for target, table in [(None, manifest), *manifest.get("target", {}).items()]:
            for section, kind in (("dependencies", "normal"), ("dev-dependencies", "dev"), ("build-dependencies", "build")):
                for name, spec in table.get(section, {}).items():
                    spec = {"version": spec} if isinstance(spec, str) else spec
                    if "version" not in spec or "git" in spec or "path" in spec:
                        raise ValueError(f"{archive}: dependency {name} is not a normalized registry dependency")
                    metadata["deps"].append({
                        "name": spec.get("package", name), "version_req": spec["version"],
                        "features": spec.get("features", []), "optional": spec.get("optional", False),
                        "default_features": spec.get("default-features", True), "target": target, "kind": kind,
                        "registry": spec.get("registry-index"), "explicit_name_in_toml": name if "package" in spec else None,
                    })
    # https://doc.rust-lang.org/cargo/reference/registry-web-api.html#publish
    description = json.dumps(metadata, separators=(",", ":")).encode()
    source = archive.read_bytes()
    return struct.pack("<I", len(description)) + description + struct.pack("<I", len(source)) + source


import argparse
import re
import shutil
import subprocess
import tempfile


def verify(source, tag):
    if not re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+', tag):
        raise ValueError('expected a production version tag')
    def git(*args):
        return subprocess.check_output(['git', *args], cwd=source, text=True).strip()
    commit = git('rev-parse', 'HEAD')
    if git('rev-parse', f'refs/tags/{tag}^{{commit}}') != commit:
        raise ValueError('source must match the selected release tag')
    subprocess.run(['git', 'merge-base', '--is-ancestor', commit, 'origin/main'], cwd=source, check=True)
    subprocess.run(['git', 'diff', '--exit-code', 'HEAD'], cwd=source, check=True)
    core = tomllib.loads((source / 'Cargo.toml').read_text())['workspace']['package']['version']
    sdk = tomllib.loads((source / 'crates/sdk/Cargo.toml').read_text())
    if tag != 'v' + core or sdk['dependencies']['e-core']['version'] != '=' + core:
        raise ValueError('release and exact core dependency must match the selected source')
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', sdk['package']['version']):
        raise ValueError('invalid SDK version')
    return commit, [('e-core', core), ('e-sdk', sdk['package']['version'])]


def prepare(source, tag, output):
    commit, packages = verify(source, tag)
    output.mkdir(parents=True, exist_ok=False)
    # A fresh target avoids Cargo's publication registry reusing an unreleased version.
    with tempfile.TemporaryDirectory(prefix='e-registry-') as target:
        import os
        env = dict(os.environ, CARGO_TARGET_DIR=target)
        subprocess.run(['cargo', 'package', '--locked', '--no-verify', '-p', 'e-core', '-p', 'e-sdk'],
                       cwd=source, env=env, check=True)
        checksums = []
        for name, version in packages:
            archive = output / f'{name}-{version}.crate'
            shutil.copyfile(Path(target) / 'package' / archive.name, archive)
            upload = archive.with_suffix('.publish')
            upload.write_bytes(payload(archive))
            for path in (archive, upload):
                checksums.append(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}')
    (output / 'source.json').write_text(json.dumps({'commit': commit, 'tag': tag,
        'packages': [{'name': name, 'version': version} for name, version in packages]}) + '\n')
    checksums.append(f"{hashlib.sha256((output / 'source.json').read_bytes()).hexdigest()}  source.json")
    (output / 'SHA256SUMS').write_text('\n'.join(checksums) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    prepare(args.source.resolve(), args.tag, args.out.resolve())
