#!/bin/sh
# Build one runner's release archives and prove the binary runs on the oldest
# glibc the release promises. The workflow retains the archives and assembles
# the release, so this script uses no `gh`.
#
# The macOS legs and the Linux legs share this script; the Linux legs run it in
# a container whose glibc *is* the floor, so the floor is a decision instead of
# whatever the runner image happens to have. Set ULO_GLIBC_CEILING to that
# version: the script refuses an image that does not match it, and refuses a
# binary that needs anything newer than it.
#
# Env: TARGETS (required), ULO_GLIBC_CEILING, and — when cutting a release — TAG,
# COMMAND, ULO_BUILD_VERSION. Without TAG it builds and checks only.
set -eu

targets=${TARGETS:?TARGETS is required}
ceiling=${ULO_GLIBC_CEILING:-}
tag=${TAG:-}

if [ -n "$ceiling" ]; then
  # A mismatch here means somebody changed the build image without revisiting
  # what the release promises, which no later check would notice.
  image=$(ldd --version 2>/dev/null | head -1 | sed -n 's/.*[^0-9]\([0-9][0-9]*\.[0-9][0-9]*\)$/\1/p')
  if [ "$image" != "$ceiling" ]; then
    echo "build image has glibc $image, not $ceiling: the release would promise a floor it does not have" >&2
    exit 1
  fi
fi

for target in $targets; do
  rustup target add "$target"
  cargo build --release --locked --target "$target"
  tar czf "ulo-$target.tar.gz" -C "target/$target/release" ulo

  if [ -n "$ceiling" ]; then
    # Every versioned GLIBC_ symbol the dynamic linker must resolve.
    required=$(
      objdump -T "target/$target/release/ulo" |
        grep -o 'GLIBC_[0-9.]*' |
        sed 's/GLIBC_//' |
        sort -Vu |
        tail -1
    )
    if [ -z "$required" ]; then
      echo "$target has no glibc symbols: is it a dynamic glibc build?" >&2
      exit 1
    fi
    newest=$(printf '%s\n%s\n' "$ceiling" "$required" | sort -V | tail -1)
    if [ "$newest" != "$ceiling" ]; then
      echo "$target needs glibc $required, newer than the $ceiling this release promises" >&2
      exit 1
    fi
    echo "$target: needs glibc $required (ceiling $ceiling)"
  fi
done

host=$(rustc -vV | sed -n 's/^host: //p')
case " $targets " in
  *" $host "*) ;;
  *) exit 0 ;;
esac
[ -n "$tag" ] || exit 0

# The release runs its own archive end to end: the binary reports the tag, the
# checksum is written, and the installer puts it in place.
"target/$host/release/ulo" --version | grep -Fx "ulo ${tag#v}"
sha256sum "ulo-$host.tar.gz" > checksums.txt 2>/dev/null ||
  shasum -a 256 "ulo-$host.tar.gz" > checksums.txt
install_dir=$(mktemp -d)
ULO_RELEASE_BASE="file://$PWD" ULO_INSTALL_DIR="$install_dir" ./install.sh \
  --version "$ULO_BUILD_VERSION"
"$install_dir/$COMMAND" --version | grep -Fx "ulo ${tag#v}"
