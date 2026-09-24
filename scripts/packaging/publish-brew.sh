#!/bin/sh
# Publish ulo and its Homebrew rename, preserving other formulas and rename entries.
set -eu
formula=$1
tap=$2
name=$(basename "$formula")
case "$name" in ulo.rb) ;; *) exit 1 ;; esac
mkdir -p "$tap/Formula"
cp "$formula" "$tap/Formula/$name.next"
python3 - "$tap/Formula/$name" "$tap/Formula/$name.next" <<'PY'
import json, pathlib, re, sys
old, new = map(pathlib.Path, sys.argv[1:])
def version(path):
    return tuple(int(x) for x in re.search(r'version "([^"]+)"', path.read_text())[1].split('.g')[0].split('.'))
if old.exists() and version(old) > version(new):
    new.unlink()
else:
    new.replace(old)
# Homebrew moves existing installations when the first ulo formula arrives.
renames = old.parent.parent / 'formula_renames.json'
mapping = json.loads(renames.read_text()) if renames.exists() else {}
mapping['e'] = 'ulo'
renames.write_text(json.dumps(mapping, indent=2, sort_keys=True) + '\n')
legacy = old.parent / 'e.rb'
if legacy.exists():
    legacy.unlink()
PY
cd "$tap"
git add "Formula/$name" formula_renames.json
if git ls-files --error-unmatch Formula/e.rb >/dev/null 2>&1; then
  git add -u -- Formula/e.rb
fi
if git diff --cached --quiet; then exit 0; fi
git -c user.name='arocomputer releases' -c user.email='support@aro.computer' commit -m "chore: update ulo to $TAG"
git push origin HEAD:main
