#!/bin/sh
# Verify a fresh registry install and its binary, retrying incomplete propagation.
set -eu
: "${VERSION:?VERSION is required}" "${COMMAND:?COMMAND is required}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
for attempt in 1 2 3 4 5 6; do
  # A successful npm install may omit an unavailable optional platform package.
  # A fresh prefix and cache prevent that incomplete install surviving a retry.
  prefix="$work/$attempt"
  if npm install --prefix "$prefix" --cache "$prefix/cache" "@arocomputer/e@$VERSION" &&
      "$prefix/node_modules/.bin/$COMMAND" --version > "$work/version" &&
      grep -Fx "e $VERSION" "$work/version"; then
    exit 0
  fi
  test "$attempt" -lt 6 || exit 1
  sleep 20
done
