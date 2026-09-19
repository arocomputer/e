#!/bin/sh
# Qualify the selected build identity and its release notes before publication.
set -eu
cd "$(dirname "$0")/.."
tag=${1:-}
if [ -n "$tag" ]; then
  python3 - "$tag" <<'PY'
import sys,tomllib
sys.path.insert(0,'scripts/release')
from identity import identity
release=identity(sys.argv[1])
m=tomllib.load(open('Cargo.toml','rb'))
base=(m.get('workspace',{}).get('package') or m['package'])['version']
assert release['channel']=='production', 'release-check expects a production tag'
assert release['version']==base, 'manifest mismatch'
PY
  ./scripts/release-notes.sh "$tag" < CHANGELOG.md | python3 -c 'import sys; sys.path.insert(0,"scripts/release"); from notes import parse; parse(sys.stdin.read())'
  export E_BUILD_VERSION=${tag#v} E_BUILD_CHANNEL=production
  export E_BUILD_COMMIT=${E_BUILD_COMMIT:-$(git rev-parse HEAD)}
fi
cargo build --release --locked
actual=$(./target/release/e --version)
if [ -n "$tag" ]; then [ "$actual" = "e ${tag#v}" ]; fi
echo "release-check: $actual qualified"
