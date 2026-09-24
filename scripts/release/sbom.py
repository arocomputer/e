#!/usr/bin/env python3
"""Generate the application's SBOM from Cargo metadata, including all target dependencies."""
import json
from pathlib import Path
import shutil
import subprocess
import sys


FILENAME = 'ulo-release-sbom'


def application_bom(metadata):
    """Find the binary package's output in either a workspace or a root package."""
    apps = [p for p in metadata['packages'] if p['name'] == 'ulo']
    if len(apps) != 1:
        raise ValueError('Expected one ulo application package')
    return Path(apps[0]['manifest_path']).parent / f'{FILENAME}.json'


def generate(destination):
    """Generate every workspace BOM, retain the application's, and remove only our outputs."""
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'], text=True))
    source = application_bom(metadata)
    outputs = [Path(p['manifest_path']).parent / f'{FILENAME}.json' for p in metadata['packages']]
    try:
        subprocess.run(['cargo', 'cyclonedx', '--format', 'json', '--spec-version', '1.5',
                        '--all', '--target', 'all', '--override-filename', FILENAME], check=True)
        bom = json.loads(source.read_text())
        if bom['metadata']['component']['name'] != 'ulo':
            raise ValueError('Generated SBOM does not describe the application')
        shutil.copyfile(source, destination)
    finally:
        for path in outputs:
            path.unlink(missing_ok=True)


if __name__ == '__main__':
    generate(Path(sys.argv[1]))
