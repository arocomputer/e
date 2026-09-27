---
title: Install
description: Build e from source, update it, and run a checkout you are editing.
order: 2
---

# Install

e is not published yet: there are no release binaries or packages. This guide
covers running it from source, which is the only way today.

## Requirements

- macOS or Linux
- Rust 1.98 or newer, from [rustup](https://rustup.rs)
- git

## Install from GitHub

```sh
cargo install --locked --git https://github.com/arocomputer/e e
```

Cargo builds the latest `main` and installs `e` into `~/.cargo/bin`, which
rustup adds to your `PATH`. Check it with `e --version`.

To update, run the same command again. `e update` and automatic updates apply
only to release builds, so a source install ignores them.

## Where e keeps its files

A build from source keeps its settings, credentials, sessions, and installed
packages in `~/.e-dev`. Release builds will use `~/.e`, the path the rest of
these guides show; until then, read `~/.e` as `~/.e-dev`. Set `E_HOME` to use
another directory. [Settings](../customize/settings.md) describes what is
inside.

## Run a checkout you are editing

To work on e itself, clone it and let `./x dev` build the checkout and start
it in a project, without installing anything:

```sh
git clone https://github.com/arocomputer/e.git
cd e
./x dev /path/to/project
```

Maintainers can also build and install an unmerged pull request as its own
`e-pr-<number>` command; [releases](../../contributing/releases.md) covers PR
previews.
