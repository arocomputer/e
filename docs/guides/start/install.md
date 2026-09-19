---
title: Install
description: Install e, keep it updated, and try a PR preview.
order: 2
---

# Install

e ships as one native binary for macOS and glibc Linux, on ARM64 and x86-64.

## Install

```sh
curl -fsSL https://e.intuitum.sh/install.sh | sh
```

The installer picks the release for your platform, verifies its checksum, and
writes the binary to `~/.local/bin`. Set `E_INSTALL_DIR` to choose another
directory. If your shell's `PATH` does not include that directory, add it.
Then check the build with `e --version`.

### Package managers

Package managers install the same binary:

```sh
brew install arocomputer/tap/e
npm install -g @arocomputer/e
bun add -g @arocomputer/e
```

These packages carry the native binary and need no JavaScript runtime. They
arrive with the first package-enabled release.

## Update

Update with the method that installed e:

```sh
e update                            # curl or a release archive
npm install -g @arocomputer/e@latest
bun add -g @arocomputer/e@latest
brew upgrade arocomputer/tap/e
```

A curl installation checks for updates at launch and installs one in the
background. To turn that off, set `auto_update` in
[settings](../customize/settings.md).

e leaves a package-managed installation to its package manager. In that case,
`e update` tells you so instead of replacing the binary.

## Try a PR preview

A pull request can be built on request without publishing anything, so you can
try an unreviewed change before it merges:

```sh
./x preview 123
```

The build is pinned to the PR's commit and installs as `e-pr-123` with its own
home, `~/.e-pr/<commit>`, kept apart from production. PR code is unreviewed; use
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
docker run --rm --entrypoint e ghcr.io/intuitums/e-slack:latest --version
```

## Build from source

Building from source needs Rust 1.98 or newer:

```sh
cargo install --git https://github.com/arocomputer/e
```

From a checkout, run `./x dev /path/to/project` to run the code you are
editing without installing it.
