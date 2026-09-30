"""Install checksum-pinned repository tools without modifying a user's PATH."""
import hashlib
import io
import json
from pathlib import Path
import platform
import tarfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def ensure(name):
    spec = json.loads((ROOT / '.github/infra-tools.json').read_text())[name]
    machine = {'x86_64': 'amd64', 'amd64': 'amd64', 'aarch64': 'arm64', 'arm64': 'arm64'}[platform.machine().lower()]
    key = f'{platform.system().lower()}-{machine}'
    asset = spec['platforms'][key]
    executable_name = name + ('.exe' if platform.system() == 'Windows' else '')
    binary = ROOT / 'target/infra-tools' / f'{name}-{spec["version"]}-{key}' / executable_name
    if binary.is_file():
        return str(binary)
    print(f'Installing {name} {spec["version"]}', flush=True)
    with urllib.request.urlopen(asset['url'], timeout=60) as response:
        archive = response.read(64 * 1024 * 1024 + 1)
        if len(archive) > 64 * 1024 * 1024:
            raise ValueError(f'{name}: archive exceeds size limit')
    if hashlib.sha256(archive).hexdigest() != asset['sha256']:
        raise ValueError(f'{name}: archive checksum mismatch')
    if asset['url'].endswith('.zip'):
        with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
            candidates = [entry for entry in bundle.infolist()
                          if not entry.is_dir() and Path(entry.filename).name == executable_name]
            if len(candidates) != 1:
                raise ValueError(f'{name}: expected one executable in archive')
            executable = bundle.read(candidates[0])
    else:
        with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as bundle:
            candidates = [entry for entry in bundle if entry.isfile() and Path(entry.name).name == executable_name]
            if len(candidates) != 1:
                raise ValueError(f'{name}: expected one executable in archive')
            executable = bundle.extractfile(candidates[0]).read()
    binary.parent.mkdir(parents=True, exist_ok=True)
    temporary = binary.with_suffix('.tmp')
    temporary.write_bytes(executable)
    temporary.chmod(0o755)
    temporary.replace(binary)
    return str(binary)
