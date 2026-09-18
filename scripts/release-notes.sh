#!/bin/sh
# Read and validate a stable draft's body before using it for publication.
set -eu
cd "$(dirname "$0")/.."

if [ "$#" -ne 1 ] || ! printf '%s\n' "$1" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'; then
  echo "usage: scripts/release-notes.sh vX.Y.Z" >&2
  exit 1
fi

release=$(gh release view "$1" --repo intuitums/e --json isDraft,body)
printf '%s\n' "$release" | python3 -c '
import json, sys
sys.path.insert(0, "scripts/release")
from notes import parse
release = json.load(sys.stdin)
if not release["isDraft"]:
    raise SystemExit("release-notes: prepare a draft release before publication")
body = release["body"]
parse(body)
sys.stdout.write(body)
'
