---
title: Install
description: Install e, keep it updated, and try preview channels.
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
npm install -g @intuitums/e
bun add -g @intuitums/e
```

These packages carry the native binary and need no JavaScript runtime. They
arrive with the first package-enabled release.

## Update

Update with the method that installed e:

```sh
e update                            # curl or a release archive
npm install -g @intuitums/e@latest
bun add -g @intuitums/e@latest
brew upgrade arocomputer/tap/e
```

A curl installation checks for updates at launch and installs one in the
background. To turn that off, set `auto_update` in
[settings](../customize/settings.md).

e leaves a package-managed installation to its package manager. In that case,
`e update` tells you so instead of replacing the binary.

## Preview channels

Preview channels let you try changes before they reach production. There are three
channels:

- **Production.** The default.
- **Beta.** A selected candidate for the next release.
- **Dev.** Follows tested changes on main.

```sh
curl -fsSL https://e.intuitum.sh/install.sh | sh -s -- --channel beta
npm install -g @intuitums/e@beta
bun add -g @intuitums/e@beta
brew install arocomputer/tap/e-beta

npm install -g @intuitums/e@dev
```

A preview build keeps its own home, `~/.e-beta` or `~/.e-dev`. Its settings
and sessions stay apart from the production installation.

How a preview installs depends on the method:

- Curl and brew install beta beside production.
- npm and bun replace the version of the one package. Install `@latest` to
  return to production.

Beta binaries come from a separate repository. An existing beta installation
needs one reinstall to adopt its new update source.

To install a specific version with the shell installer, pass
`--version X.Y.Z`. For a beta version, also pass `--channel beta`.

[Releases and testing](../../../contributing/releases.md) covers every
installer, the local `./x dev` build, and PR builds.

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
