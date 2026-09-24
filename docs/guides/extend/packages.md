---
title: Packages
description: Install and share extensions, skills, prompts, and themes.
order: 2
---

# Packages

A package bundles extensions, skills, prompt templates, and themes so you
can share them.

A package is an npm package, a git repository, or a directory on disk. It is
laid out like `~/.ulo/` itself. It holds any subset of these four directories
and needs nothing else:

```
extensions/   executables, or bundle directories with an entry point
skills/       <name>/SKILL.md folders
prompts/      <name>.md templates
themes/       <name>.json palettes
```

ulo has no manifest of its own. A `package.json` belongs to npm. ulo reads the
directories and takes only the dependency list from the manifest.

Find packages in the [catalog](https://ulo.sh/docs/catalog/) or on
npmjs.com. The catalog lists every npm package that carries the `ulo-package`
keyword.

> [!WARNING]
> A package runs with your full permissions. Its extensions are executables ulo
> starts at launch, and its skills and prompts steer the model. Read the source
> before installing anything, and pin a ref you have read.

## Install and manage

Use `ulo install`, `ulo packages`, and `ulo remove` to manage the packages you
use.

```sh
ulo install npm:@fschrhunt1/ulo-diff                # from npm, follows latest
ulo install npm:@team/ulo-tools@1.4.0               # pinned to a version
ulo install git:github.com/fschrhunt/ulo-diff       # a git repository, default branch
ulo install git:github.com/fschrhunt/ulo-diff@v2    # pin a tag, branch, or commit
ulo install https://github.com/user/repo          # any git URL works
ulo install git:git@github.com:user/repo@main     # SSH, with your keys
ulo install ~/src/my-package                      # a local directory, in place

ulo packages                                      # what is listed, and its state
ulo remove npm:@fschrhunt1/ulo-diff                 # forget it, delete the install
ulo install                                       # make disk match settings
```

Installing records the source in the `packages` list of
`~/.ulo/settings.json`. Restart ulo or run `/reload` to pick up the package. The
files go here:

| Source | Install location |
| --- | --- |
| npm | `~/.ulo/packages/npm/node_modules/<name>` |
| git | `~/.ulo/packages/<host>/<path>` |

The npm directory is one npm project that belongs to ulo. Never edit it by
hand.

### npm and dependencies

Installing a package runs nothing. Only loading it does. `ulo install` passes
`--ignore-scripts` to npm every time, so npm never runs a package's
lifecycle scripts.

A git package whose `package.json` declares `dependencies` gets them
installed the same way. ulo runs `npm ci` when there is a lockfile and
`npm install` when there is not. An extension that imports a library works
after one `ulo install`.

Both npm and git packages with dependencies need `npm` on `PATH`. They use
npm's registry and credentials as you have configured them.

### Git access

Git runs as a subprocess, so ulo can install whatever your `git` can clone.
That includes private hosts, SSH config, and credential helpers. Git never
prompts under ulo. The same goes for `npm` and its registry settings.

### Settings are the source of truth

The `packages` list in settings decides what is installed, not the directory
on disk.

- `ulo install` with no source installs every listed package that is missing
  and brings the rest current. An unpinned npm package moves to `latest`,
  and a pinned one stays. A pinned git ref is checked out again, and an
  unpinned one fast-forwards to its default branch.
- To restore your set on a new machine, copy `settings.json` and run
  `ulo install` once.
- `ulo install <same package>@<other ref>` moves the pin. The list keeps one
  entry, not two.
- Package identity ignores scheme, credentials, `.git`, and host case. The
  HTTPS and SSH spellings of one repository are the same package.
- If a listed package is not on disk, ulo reports it once in the transcript at
  startup. Startup itself never touches the network. Only `ulo install` does.
- ulo references a local directory where it is and never copies it. `ulo remove`
  only forgets it.

## Try a package for one run

Use `ulo --package <source> …`, or `-P`, to load a package for this run only.
This is how you try a package before installing it, or run one from a
checkout you are editing.

- A directory loads in place.
- A git source loads from a temporary clone.
- A release asset loads from a temporary directory.

Nothing is recorded, and clones are removed at exit. Repeat the flag to load
several packages.

## Share packages with a team

A trusted repository can list the packages its team shares in a
`.ulo/packages` file. Put one source per line. Lines starting with `#` are
comments.

When you trust the directory, ulo offers to install the ones you lack. These
packages behave like the entries in your settings:

- They install on `ulo install`.
- They show in `ulo packages`.
- ulo reports them at startup when they are missing.

ulo never writes them into your settings.

Only npm, git, and release sources count in this file. ulo ignores a local
directory line, because trusting a checkout must not be enough to run code
it carries in place. If you mean to use one, install it yourself with
`ulo install ./dir`.

## Load part of a package

To load only some of a package, write its `packages` entry as an object
instead of a string. The object holds the `source` plus a glob list per kind
that names what loads.

```json
{
  "packages": [
    "npm:@fschrhunt1/ulo-diff",
    {
      "source": "npm:@team/ulo-tools@1.4.0",
      "extensions": ["!extensions/legacy.mjs"],
      "prompts": ["prompts/review.md", "prompts/r*.md"]
    }
  ]
}
```

Patterns are relative to the package root, and `!` excludes. The lists work
like this:

- A kind with no list loads whole.
- A list of exclusions alone loads everything else.
- Once a list has an inclusion, only what an inclusion names loads, minus
  the exclusions.

Names are the top-level entries of the kind's directory. Those are an
extension file or bundle directory, a skill folder, or a prompt or theme
file. When `ulo install` moves a pin, it keeps the filters.

## How package resources load

Every loader reads `~/.ulo/<kind>/` first. It then reads each installed
package's `<kind>/` in settings order.

Skills and prompts go one step further. After `/trust`, ulo also reads the
repository's own `.ulo/skills/` and `.ulo/prompts/`. Extensions and themes never
load from a repository, because trusting a checkout must not run its code or
restyle your terminal. If you mean to use them, install the repository as a
package.

On a name clash, the closer context wins. A repo skill shadows a global one,
and a global one shadows a package's. A package theme can name a built-in
theme such as `dark` and replace it, unless `~/.ulo/themes/dark.json` exists.

Each kind shows up in its usual place:

| Kind | Where it appears |
| --- | --- |
| Extensions | Launch like any in `~/.ulo/extensions/`. Their config lives under their own name in `"extensions"` in `settings.json`. |
| Skills | Show `Package` as their scope in the `$` picker. |
| Prompts | Become `/name` commands. |
| Themes | Appear under Theme in `/settings`. |

## Release packages

A compiled extension can install from a GitHub release:

```sh
ulo install release:<owner>/<repo>/<name>        # the latest release
ulo install release:<owner>/<repo>/<name>@v1.2.0 # pinned
```

ulo downloads `<name>-<target>.tar.gz` for this machine's platform from the
release. It checks the file against the release's `checksums.txt`. It then
places the `<name>` executable under
`~/.ulo/packages/releases/<owner>/<repo>/<name>/extensions/`, the same shape
as every other package.

An unpinned release package follows the latest release on `ulo install`.
`ulo remove` deletes it. The supported platforms are the ones ulo itself is
released for.

### Publish a release package

To publish a release package:

1. Name the asset `<name>-<target>.tar.gz`, with the executable at its top
   level.
2. List the asset in the release's `checksums.txt`, using `sha256sum`.
3. Build one asset per target ulo is released for. `ulo update` names the
   targets.

The ulo repository ships no packages of its own, so nothing about a package
needs a change to ulo.

## Publish a package

Create a package, try it in place, then publish it to npm:

```sh
ulo packages init my-package     # the directories, one extension, package.json, README
cd my-package && ulo --package . # try it in place
npm publish                    # ulo install npm:my-package for everyone
```

`ulo packages init` writes a `package.json` with two things:

- The `ulo-package` keyword. That keyword is what lists a package in the
  catalog.
- A `files` list naming the four directories, so nothing else ships.

Add the kinds you ship as keywords too: `extensions`, `skills`, `prompts`,
or `themes`. The catalog labels the package with them.

Keep extensions executable with `chmod +x`, and commit the mode. npm and git
both carry it. Tag or version your releases so users can pin what they read.

A git repository with the same layout is installable as
`git:<host>/<user>/<repo>` without any of this. Publishing to npm is what
makes a package findable.

[fschrhunt/ulo-diff](https://github.com/fschrhunt/ulo-diff) shows the shape. It
holds one extension file and one prompt template, and a single `ulo install`
installs it. This package is how `/diff` reaches ulo. The command comes from a
package, not from ulo itself.
