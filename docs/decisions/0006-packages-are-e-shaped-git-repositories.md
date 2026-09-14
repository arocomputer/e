# Packages are e-shaped git repositories

Status: accepted
Date: 2026-09-13

## Context

Extensions, skills, prompt templates, and themes were each a file the user
copied into `~/.e/` by hand. Nothing could be shared, versioned, listed, or
removed, and nothing gave anyone a reason to build for e rather than into
it. Comparable agents (pi) solved this with a package concept: an npm or git
source carrying convention directories, recorded in settings, with install,
remove, update, and a gallery. Adopting a registry would make e depend on
a package manager it does not otherwise need, and a manifest would be a
second description of what the directories already say.

## Decision

A package is a git repository, or a local directory, laid out like `~/.e/`
itself: any subset of `extensions/`, `skills/`, `prompts/`, `themes/`, no
manifest. `e install <source>` clones it with the user's own `git` under
`~/.e/packages/<host>/<path>` and records the source string, as typed, in
the `packages` list of `settings.json`. Local directories are referenced in
place. Settings are the source of truth: `e install` with no argument makes
disk match them, and a listed package missing on disk is reported at startup
without any network access.

Every loader reads `~/.e/<kind>/`, then each package's `<kind>/` in settings
order, then a trusted repository's `.e/<kind>/`. The closer context wins a
name clash. Packages are user scope only; a repository's `.e/` remains the
project-scoped form and carries no installs.

Discovery is a GitHub topic (`e-package`), not a hosted gallery.

## Consequences

- The four kinds share one distribution story and one shape. A fifth kind
  joins by adding a directory name, not a format.
- git is a runtime requirement of `e install`, and only of it. The binary's
  own network surface is unchanged; the guard's host list does not move.
- `~/.e/packages/` and the `packages` settings key are persisted contracts
  under `docs/compatibility.md`.
- Filtering what a package loads, project-scoped installs, and try-once
  installs are not provided. Each is a settings-shape change to be made
  deliberately when a package needs it.
