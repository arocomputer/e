#!/bin/sh
# Update only e's formula; skip older reruns and preserve other tap changes.
set -eu
formula=$1
tap=$2
name=$(basename "$formula")
case "$name" in e.rb|e-beta.rb|e-dev.rb) ;; *) exit 1 ;; esac
mkdir -p "$tap/Formula"
cp "$formula" "$tap/Formula/$name.next"
python3 - "$tap/Formula/$name" "$tap/Formula/$name.next" <<'PY'
import pathlib, re, sys
old, new = map(pathlib.Path, sys.argv[1:])
def version(path):
    return tuple(int(x) for x in re.search(r'version "([^"]+)"', path.read_text())[1].split('.g')[0].replace('-dev', '').replace('-beta', '').split('.'))
if old.exists() and version(old) > version(new):
    new.unlink()
else:
    new.replace(old)
PY
cd "$tap"
git add "Formula/$name"
if git diff --cached --quiet; then exit 0; fi
git -c user.name='Intuitum releases' -c user.email='support@intuitum.sh' commit -m "chore: update e to $TAG"
git push origin HEAD:main
