#!/bin/sh
# Print one tagged version's changelog body from stdin, without release chrome.
# Missing, empty, or duplicate sections fail before any notes reach stdout.
set -eu

if [ "$#" -ne 1 ] || ! printf '%s\n' "$1" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'; then
  echo "usage: scripts/release-notes.sh vX.Y.Z < CHANGELOG.md" >&2
  exit 1
fi

awk -v version="${1#v}" '
  /^## / {
    active = ($0 == "## " version)
    if (active) found++
    next
  }
  active {
    notes = notes $0 "\n"
    if ($0 ~ /[^[:space:]]/) nonempty = 1
  }
  END {
    if (found != 1 || !nonempty) exit 1
    printf "%s", notes
  }
' || {
  echo "release-notes: expected one nonempty section for ${1#v}" >&2
  exit 1
}
