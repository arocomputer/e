#!/bin/sh
# Install generated tarballs through npm and bun with lifecycle scripts disabled.
set -eu
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
version=${E_SMOKE_VERSION:-1.2.3}
command=${E_SMOKE_COMMAND:-e}
python3 - "$scratch" "v$version" <<'PY'
import importlib.util, json, pathlib, subprocess, sys
sys.path.insert(0, 'scripts/packaging')
from test_prepare import Packages
from prepare import prepare, PLATFORMS
fixture = Packages()
fixture.setUp()
root = pathlib.Path(sys.argv[1])
try:
    prepare(sys.argv[2], fixture.assets, root / 'dist')
    dependencies = {}
    for platform in PLATFORMS:
        result = json.loads(subprocess.check_output(['npm','pack', str(root/'dist'/platform), '--json','--pack-destination',str(root)]))
        packed = list(result.values())[0] if isinstance(result, dict) else result[0]
        dependencies['@arocomputer/e-'+platform] = 'file:' + str(root / packed['filename'])
    wrapper = root/'dist/e/package.json'
    data = json.loads(wrapper.read_text())
    data['optionalDependencies'] = dependencies
    wrapper.write_text(json.dumps(data))
    subprocess.check_call(['npm','pack',str(wrapper.parent),'--pack-destination',str(root)], stdout=subprocess.DEVNULL)
finally:
    fixture.tearDown()
PY
npm install --global --prefix "$scratch/npm" --ignore-scripts --no-audit --no-fund "$scratch/arocomputer-e-$version.tgz"
test "$("$scratch/npm/bin/$command" 'argument with spaces')" = 'argument with spaces'
cat > "$scratch/bunfig.toml" <<CFG
[install]
globalDir = "$scratch/bun/global"
globalBinDir = "$scratch/bun/bin"
CFG
bun install --global --config="$scratch/bunfig.toml" --ignore-scripts "$scratch/arocomputer-e-$version.tgz"
test "$("$scratch/bun/bin/$command" 'argument with spaces')" = 'argument with spaces'
echo 'npm and bun launch the native dependency without lifecycle scripts'
