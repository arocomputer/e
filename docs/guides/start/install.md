---
title: Install
description: installation methods, updates, and preview channels
order: 2
---

# Install

e ships as one native binary for macOS and glibc Linux, on ARM64 and x86-64.

## Install

```sh
curl -fsSL https://e.intuitum.sh/install.sh | sh
```

The installer picks the release for your platform, verifies its checksum, and
writes the binary to `~/.local/bin`; `E_INSTALL_DIR` chooses another directory.
Add that directory to `PATH` if your shell does not already have it, then check
the build with `e --version`.

Package managers install the same binary:

```sh
brew install intuitums/tap/e
npm install -g @intuitums/e
bun add -g @intuitums/e
```

These carry the native binary and need no JavaScript runtime, and they arrive
with the first package-enabled release.

## Update

Update with the method that installed e:

```sh
e update                            # curl or a release archive
npm install -g @intuitums/e@latest
bun add -g @intuitums/e@latest
brew upgrade intuitums/tap/e
```

A curl installation checks for updates at launch and installs one in the
background; `auto_update` in [settings](../customize/settings.md) turns that
off. A package-managed installation is left to its package manager, and
`e update` says so instead of replacing it.

## Preview channels

Stable is the default. Beta is a selected candidate for the next release, and
dev follows tested changes on main.

```sh
curl -fsSL https://e.intuitum.sh/install.sh | sh -s -- --channel beta
npm install -g @intuitums/e@beta
bun add -g @intuitums/e@beta
brew install intuitums/tap/e-beta

npm install -g @intuitums/e@dev
```

A preview build keeps its own home — `~/.e-beta` or `~/.e-dev` — so its
settings and sessions stay apart from the stable installation. Curl and brew
install beta beside production; npm and bun replace the version of the one
package, and `@latest` returns it to stable. Beta binaries come from a separate
repository, so an existing beta needs one reinstall to adopt its new update
source. [Releases and testing](../../../contributing/releases.md) covers every
installer, the local `./x dev` build, and PR builds.

A specific version is `--version X.Y.Z` on the shell installer; a beta version
also needs `--channel beta`.

## Requirements

The Linux binaries link against glibc 2.39 or newer — Ubuntu 24.04+, Debian 13+,
Fedora 40+, RHEL 10+. An older distribution needs a build from source, or the
published image, which carries its own runtime:

```sh
docker run --rm --entrypoint e ghcr.io/intuitums/e-slack:latest --version
```

## Build from source

Rust 1.98 or newer:

```sh
cargo install --git https://github.com/intuitums/e
```

From a checkout, `./x dev /path/to/project` runs the code you are editing
without installing it.
