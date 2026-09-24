---
title: Install
description: Install ulo, keep it updated, and try a PR preview.
order: 2
---

# Install

ulo ships as one native binary for macOS and glibc Linux, on ARM64 and x86-64.

## Install

```sh
curl -fsSL https://ulo.sh/install.sh | sh
```

The installer picks the release for your platform, verifies its checksum, and
writes the binary to `~/.local/bin`. Set `ULO_INSTALL_DIR` to choose another
directory. If your shell's `PATH` does not include that directory, add it.
Then check the build with `ulo --version`.

### Package managers

Package managers install the same binary:

```sh
brew install arocomputer/tap/ulo
npm install -g @arocomputer/ulo
bun add -g @arocomputer/ulo
```

These packages carry the native binary and need no JavaScript runtime. They
arrive with the first package-enabled release.

## Migrate an existing installation

The product was previously named `e`. Install `ulo` with the commands above,
then update command invocations and `E_*` environment variables to `ULO_*`.
The explicit configuration override is now `ULO_HOME`.

ulo prefers `~/.ulo`. If it is absent, an existing `~/.e` remains the active
store, including settings, credentials, sessions, and installed packages.
Local and PR builds apply the same rule to their `-dev` and `-pr` directories.
Trusted workspace resources prefer `.ulo` and fall back to `.e`.

To move the store, stop running sessions first and run this only when the
destination does not exist:

```sh
test ! -e "$HOME/.ulo" && mv "$HOME/.e" "$HOME/.ulo"
```

The directories are never merged automatically. If both exist, select the
one you want with `ULO_HOME`. Update extension scripts that refer to the old
path or environment variables. Existing session and configuration formats
remain readable.

## Update

Update with the method that installed ulo:

```sh
ulo update                            # curl or a release archive
npm install -g @arocomputer/ulo@latest
bun add -g @arocomputer/ulo@latest
brew upgrade arocomputer/tap/ulo
```

A curl installation checks for updates at launch and installs one in the
background. To turn that off, set `auto_update` in
[settings](../customize/settings.md).

ulo leaves a package-managed installation to its package manager. In that case,
`ulo update` tells you so instead of replacing the binary.

## Try a PR preview

A pull request can be built on request without publishing anything, so you can
try an unreviewed change before it merges:

```sh
./x preview 123
```

The build is pinned to the PR's commit and installs as `ulo-pr-123` with its own
home, `~/.ulo-pr/<commit>`, kept apart from production. PR code is unreviewed; use
a disposable project. [Releases and testing](../../../contributing/releases.md)
covers the preview workflow and the local `./x dev` build.

To install a specific production version with the shell installer, pass
`--version X.Y.Z`. npm and bun accept the exact version after `@`.

## Requirements

The Linux binaries link against glibc 2.39 or newer. That means Ubuntu 24.04+,
Debian 13+, Fedora 40+, or RHEL 10+.

On an older distribution, build from source or use the published image. The
image carries its own runtime:

```sh
docker run --rm --entrypoint ulo ghcr.io/intuitums/ulo-slack:latest --version
```

## Build from source

Building from source needs Rust 1.98 or newer:

```sh
cargo install --git https://github.com/arocomputer/ulo
```

From a checkout, run `./x dev /path/to/project` to run the code you are
editing without installing it.
