#!/bin/sh
# Update only e's formula; skip older reruns and preserve other tap changes.
set -eu
formula=$1
tap=$2
mkdir -p "$tap/Formula"
cp "$formula" "$tap/Formula/e.rb.next"
python3 - "$tap/Formula/e.rb" "$tap/Formula/e.rb.next" <<'PY'
import pathlib, re, sys
old, new = map(pathlib.Path, sys.argv[1:])
def version(path):
    return tuple(map(int, re.search(r'version "([0-9.]+)"', path.read_text())[1].split('.')))
if old.exists() and version(old) > version(new):
    new.unlink()
else:
    new.replace(old)
PY
cd "$tap"
git add Formula/e.rb
if git diff --cached --quiet; then exit 0; fi
git -c user.name='Intuitum releases' -c user.email='support@intuitum.sh' commit -m "chore: update e to $TAG"
git push origin HEAD:main
