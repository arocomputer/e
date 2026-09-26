---
title: Install
description: Install e, keep it updated, and try a PR preview.
order: 2
---

# Install

e is in development. Public releases, npm packages, and the Homebrew formula
have been withdrawn. Use the Rust toolchain to build it locally.

## Install

```sh
git clone https://github.com/arocomputer/e.git
cd e
./x dev /path/to/project
```

`./x dev` builds the current checkout and runs it in the selected project.
Local builds use `~/.e-dev` and do not self-update. To inspect the build,
run `cargo run -- --version` from the checkout.

### Package managers

Package-manager installations are unavailable during development. The npm
scope for the first public release has not been finalized.

## Migrate an existing installation

The product was briefly named `ulo`. Build `e` with the commands above,
then update command invocations and `ULO_*` environment variables to `E_*`.
The explicit configuration override is now `E_HOME`.

e prefers `~/.e`. If it is absent, an existing `~/.ulo` remains the active
store, including settings, credentials, sessions, and installed packages.
Local and PR builds apply the same rule to their `-dev` and `-pr` directories.
Trusted workspace resources prefer `.e` and fall back to `.ulo`.

To move the store, stop running sessions first and run this only when the
destination does not exist:

```sh
test ! -e "$HOME/.e" && mv "$HOME/.ulo" "$HOME/.e"
```

The directories are never merged automatically. If both exist, select the
one you want with `E_HOME`. Update extension scripts that refer to the old
path or environment variables. Existing session and configuration formats
remain readable.

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
a disposable project. [Releases and testing](../../contributing/releases.md)
covers the preview workflow and the local `./x dev` build.

To install a specific production version with the shell installer, pass
`--version X.Y.Z`. npm and bun accept the exact version after `@`.

## Requirements

The Linux binaries link against glibc 2.39 or newer. That means Ubuntu 24.04+,
Debian 13+, Fedora 40+, or RHEL 10+.

On an older distribution, build from source or use the published image. The
image carries its own runtime:

```sh
docker run --rm --entrypoint e ghcr.io/arocomputer/e-slack:latest --version
```

## Build from source

Building from source needs Rust 1.98 or newer:

```sh
cargo install --git https://github.com/arocomputer/e
```

From a checkout, run `./x dev /path/to/project` to run the code you are
editing without installing it.
