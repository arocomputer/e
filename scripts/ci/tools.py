"""Install checksum-pinned repository tools without modifying a user's PATH."""
import hashlib
import io
import json
from pathlib import Path
import platform
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


def ensure(name):
    spec = json.loads((ROOT / '.github/infra-tools.json').read_text())[name]
    machine = {'x86_64': 'amd64', 'aarch64': 'arm64', 'arm64': 'arm64'}[platform.machine()]
    key = f'{platform.system().lower()}-{machine}'
    asset = spec['platforms'][key]
    binary = ROOT / 'target/infra-tools' / f'{name}-{spec["version"]}-{key}' / name
    if binary.is_file():
        return str(binary)
    print(f'Installing {name} {spec["version"]}', flush=True)
    with urllib.request.urlopen(asset['url'], timeout=60) as response:
        archive = response.read()
    if hashlib.sha256(archive).hexdigest() != asset['sha256']:
        raise ValueError(f'{name}: archive checksum mismatch')
    with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as bundle:
        candidates = [entry for entry in bundle if entry.isfile() and Path(entry.name).name == name]
        if len(candidates) != 1:
            raise ValueError(f'{name}: expected one executable in archive')
        executable = bundle.extractfile(candidates[0]).read()
    binary.parent.mkdir(parents=True, exist_ok=True)
    temporary = binary.with_suffix('.tmp')
    temporary.write_bytes(executable)
    temporary.chmod(0o755)
    temporary.replace(binary)
    return str(binary)
