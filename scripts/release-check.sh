#!/bin/sh
# Qualify the selected build identity and its release notes before publication.
set -eu
cd "$(dirname "$0")/.."
tag=${1:-}
channel=${E_BUILD_CHANNEL:-stable}
if [ -n "$tag" ]; then
  python3 - "$tag" "$channel" <<'PY'
import sys,tomllib
sys.path.insert(0,'scripts/release')
from identity import identity
release=identity(sys.argv[1])
base=tomllib.load(open('Cargo.toml','rb'))['package']['version']
assert release['channel']==sys.argv[2], 'channel mismatch'
assert release['version'].split('-')[0]==base, 'manifest mismatch'
PY
  if [ "$channel" = stable ]; then
    ./scripts/release-notes.sh "$tag" < CHANGELOG.md | python3 -c 'import sys; sys.path.insert(0,"scripts/release"); from notes import parse; parse(sys.stdin.read())'
  fi
  export E_BUILD_VERSION=${tag#v} E_BUILD_CHANNEL=$channel
  export E_BUILD_COMMIT=${E_BUILD_COMMIT:-$(git rev-parse HEAD)}
fi
cargo build --release --locked
actual=$(./target/release/e --version)
if [ -n "$tag" ]; then [ "$actual" = "e ${tag#v}" ]; fi
echo "release-check: $actual qualified"
