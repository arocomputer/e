#!/bin/sh
# ulo installer: fetch the latest release binary for this platform, verify its
# checksum, install to ~/.local/bin (override with ULO_INSTALL_DIR).
#   curl -fsSL https://ulo.sh/install.sh | sh
set -eu

repo="arocomputer/ulo"
dir="${ULO_INSTALL_DIR:-$HOME/.local/bin}"

version=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --version) version=${2:?version required}; shift 2 ;;
    *) echo 'usage: install.sh [--version X.Y.Z]' >&2; exit 2 ;;
  esac
done
if [ -n "$version" ]; then
  version=${version#v}
  printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || { echo 'version must be X.Y.Z' >&2; exit 2; }
fi

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Darwin)
    case "$arch" in
      arm64)  target="aarch64-apple-darwin" ;;
      x86_64) target="x86_64-apple-darwin" ;;
      *) echo "unsupported macOS architecture: $arch" >&2; exit 1 ;;
    esac ;;
  Linux)
    case "$arch" in
      aarch64|arm64) target="aarch64-unknown-linux-gnu" ;;
      x86_64)        target="x86_64-unknown-linux-gnu" ;;
      *) echo "unsupported Linux architecture: $arch" >&2; exit 1 ;;
    esac ;;
  *) echo "unsupported platform: $os" >&2; exit 1 ;;
esac

# The Linux binaries are linked against the glibc of the image that built them
# (Debian 11, 2.31), so a system below that cannot run them at all. Refuse here,
# where the reason is nameable, instead of letting the linker fail after the
# download. The floor is overridable so the refusal is testable on any host.
if [ "$os" = Linux ]; then
  required=${ULO_INSTALL_GLIBC:-2.31}
  present=$(ldd --version 2>/dev/null | head -1 | sed -n 's/.*[^0-9]\([0-9][0-9]*\.[0-9][0-9]*\)$/\1/p')
  if [ -n "$present" ] && [ "$(printf '%s\n%s\n' "$required" "$present" | sort -V | head -1)" != "$required" ]; then
    echo "ulo's Linux binaries need glibc $required or newer; this system has $present." >&2
    echo 'Debian 11+, Ubuntu 22.04+, and RHEL 9+ have it. Otherwise run the published image,' >&2
    echo 'which carries its own runtime: docker run --rm --entrypoint ulo ghcr.io/intuitums/ulo-slack:latest --version' >&2
    exit 1
  fi
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# ULO_RELEASE_BASE is an internal release-smoke seam: production installs leave
# it unset; CI points it at the just-built local artifacts.
base="https://github.com/$repo/releases/latest/download"
if [ -n "$version" ]; then base="https://github.com/$repo/releases/download/v$version"; fi
base=${ULO_RELEASE_BASE:-$base}

curl -fsSL -o "$tmp/ulo.tar.gz" "$base/ulo-$target.tar.gz" || {
  echo "no release published yet — install.sh works once the first release exists" >&2
  echo "build from source: cargo install --git https://github.com/arocomputer/ulo" >&2
  exit 1
}
curl -fsSL -o "$tmp/checksums.txt" "$base/checksums.txt"

cd "$tmp"
expected=$(grep " ulo-$target.tar.gz$" checksums.txt | cut -d' ' -f1)
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum ulo.tar.gz | cut -d' ' -f1)
else
  actual=$(shasum -a 256 ulo.tar.gz | cut -d' ' -f1)
fi
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  echo "checksum mismatch — refusing to install" >&2
  exit 1
fi

tar xzf ulo.tar.gz ulo
[ -f ulo ] && [ ! -L ulo ] || { echo "invalid executable" >&2; exit 1; }
if [ -n "$version" ] && [ "$(./ulo --version)" != "ulo $version" ]; then
  echo "archive identity does not match requested version" >&2
  exit 1
fi
mkdir -p "$dir"
# Refuse to replace package-owned executables, including symlinks into their stores.
if [ -L "$dir/ulo" ] || [ -e "$dir/.ulo-install-method" ]; then
  echo "destination is package-managed; choose another ULO_INSTALL_DIR" >&2
  exit 1
fi
install -m 755 ulo "$dir/.ulo.next"
mv -f "$dir/.ulo.next" "$dir/ulo"

echo "installed $("$dir/ulo" --version) to $dir/ulo"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "note: $dir is not on your PATH" ;;
esac
