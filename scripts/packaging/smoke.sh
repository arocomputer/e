#!/bin/sh
# Install generated tarballs through npm and Bun with lifecycle scripts disabled.
set -eu
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
python3 - "$scratch" <<'PY'
import importlib.util, json, pathlib, subprocess, sys
sys.path.insert(0, 'scripts/packaging')
from test_prepare import Packages
from prepare import prepare, PLATFORMS
fixture = Packages()
fixture.setUp()
root = pathlib.Path(sys.argv[1])
try:
    prepare('v1.2.3', fixture.assets, root / 'dist')
    dependencies = {}
    for platform in PLATFORMS:
        result = json.loads(subprocess.check_output(['npm','pack', str(root/'dist'/platform), '--json','--pack-destination',str(root)]))
        packed = list(result.values())[0] if isinstance(result, dict) else result[0]
        dependencies['@intuitums/e-'+platform] = 'file:' + str(root / packed['filename'])
    wrapper = root/'dist/e/package.json'
    data = json.loads(wrapper.read_text())
    data['optionalDependencies'] = dependencies
    wrapper.write_text(json.dumps(data))
    subprocess.check_call(['npm','pack',str(wrapper.parent),'--pack-destination',str(root)], stdout=subprocess.DEVNULL)
finally:
    fixture.tearDown()
PY
npm install --global --prefix "$scratch/npm" --ignore-scripts --no-audit --no-fund "$scratch/intuitums-e-1.2.3.tgz"
test "$("$scratch/npm/bin/e" 'argument with spaces')" = 'argument with spaces'
cat > "$scratch/bunfig.toml" <<CFG
[install]
globalDir = "$scratch/bun/global"
globalBinDir = "$scratch/bun/bin"
CFG
bun install --global --config="$scratch/bunfig.toml" --ignore-scripts "$scratch/intuitums-e-1.2.3.tgz"
test "$("$scratch/bun/bin/e" 'argument with spaces')" = 'argument with spaces'
echo 'npm and Bun launch the native dependency without lifecycle scripts'
