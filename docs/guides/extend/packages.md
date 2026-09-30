---
title: Packages
description: Install and share extensions, skills, prompts, and themes.
order: 2
---

# Packages

A package bundles extensions, skills, prompt templates, and themes so they
can be installed with one command. Read this guide to install someone
else's package or to publish your own.

> [!WARNING]
> A package runs with your full permissions. Its extensions are executables e
> starts at launch, and its skills and prompts steer the model. Read the source
> before installing anything, and pin a ref you have read.

## What a package is

A package is an npm package, a git repository, a GitHub release asset, or a
directory on disk. It is laid out like `~/.e/` and holds any subset of four
directories:

```
extensions/   executables, or bundle directories with an entry point
skills/       <name>/SKILL.md folders
prompts/      <name>.md templates
themes/       <name>.json palettes
```

e has no package manifest of its own. It reads the directories; a
`package.json`, if present, belongs to npm, and e uses only its dependency
list.

## Install and manage

These examples show source syntax; replace the sample names with real packages.

```sh
e install npm:@team/e-tools                # from npm, follows latest
e install npm:@team/e-tools@1.4.0               # pinned to a version, range, or dist-tag
e install git:github.com/team/e-tools       # a git repository, default branch
e install git:github.com/team/e-tools@v2    # pinned to a tag, branch, or commit
e install https://github.com/user/repo          # any git URL (https, ssh, git, file)
e install git:git@github.com:user/repo@main     # SSH, with your keys
e install release:owner/repo/name@v1.2.0        # a compiled extension from a GitHub release
e install ~/src/my-package                      # a local directory, used in place

e packages                                      # each listed package and its state
e remove npm:@team/e-tools                 # forget it and delete the install
e install                                       # make disk match settings
```

A local directory is `.`, `..`, or a path starting with `/`, `./`, `../`,
or `~/`. Installing
records the source in the `packages` list of `~/.e/settings.json` (see
[settings](../customize/settings.md) for where your build keeps its home).
Restart e or run `/reload` to load the package. The files go here:

| Source | Install location |
| --- | --- |
| npm | `~/.e/packages/npm/node_modules/<name>` |
| git | `~/.e/packages/<host>/<path>` |
| release | `~/.e/packages/releases/<owner>/<repo>/<name>` |
| local | where it is; never copied |

The npm directory is one npm project that belongs to e. Do not edit it by
hand.

### Settings are the source of truth

The `packages` list in settings decides what is installed, not the
directory on disk.

- `e install` with no source installs every listed package that is missing
  and updates the rest. Unpinned npm packages move to `latest`, unpinned git
  packages fast-forward their default branch, and unpinned release packages
  follow the latest release. Pinned packages stay at, or return to, their
  pin.
- To restore your set on a new machine, copy `settings.json` and run
  `e install`.
- `e install <package>@<other ref>` moves the pin; the list keeps one entry.
- Package identity ignores scheme, credentials, `.git`, and host case, so
  the HTTPS and SSH spellings of one repository are one package.
- A listed package missing from disk is reported once in the transcript at
  startup. Startup never touches the network; only `e install` does.
- `e remove` on a local directory only forgets it.

### npm, git, and dependencies

e runs `git` and `npm` as subprocesses, so it installs whatever your `git`
can clone, including private hosts, SSH config, and credential helpers, and
uses npm's registry and credentials as you configured them. Git never
prompts: a source that needs interactive credentials fails instead.

Installing runs no package code. e passes `--ignore-scripts` to every npm
command, so lifecycle scripts never run. A git package whose `package.json`
declares `dependencies` gets them installed the same way, with `npm ci`
when there is a lockfile and `npm install` otherwise. npm packages and git
packages with dependencies need `npm` on `PATH`.

## Try a package for one run

`e --package <source>`, or `-P`, loads a package for this run only, without
recording it. Use it to try a package before installing it, or to run one
from a checkout you are editing. A directory loads in place; npm, git, and
release sources load from a temporary directory that is removed at exit.
Repeat the flag to load several packages.

## Load part of a package

To load only part of a package, write its `packages` entry as an object
with the `source` and a glob list per kind (`extensions`, `skills`,
`prompts`, `themes`):

```json
{
  "packages": [
    "npm:@team/e-tools",
    {
      "source": "npm:@team/e-tools@1.4.0",
      "extensions": ["!extensions/legacy.mjs"],
      "prompts": ["prompts/review.md", "prompts/r*.md"]
    }
  ]
}
```

Patterns are relative to the package root, and `!` excludes. They match the
top-level entries of the kind's directory: an extension file or bundle
directory, a skill folder, or a prompt or theme file.

- A kind with no list loads whole.
- A list of exclusions alone loads everything else.
- Once a list has an inclusion, only what an inclusion names loads, minus
  the exclusions.

When `e install` moves a pin, it keeps the filters.

## Share packages with a team

A repository can list the packages its team uses in `.e/packages`: one
source per line, with `#` for comments. When you
[trust](../customize/instructions.md) the directory, e offers to install the ones you lack. After that they
behave like entries in your settings (they install on `e install`, show in
`e packages`, and are reported when missing), but e never writes them into
your settings.

Only npm, git, and release sources count in this file. e ignores local
directory lines, because trusting a checkout must not be enough to run code
it carries. To use one, install it yourself with `e install ./dir`.

## How package resources load

Every loader reads `~/.e/<kind>/` first, then each package's `<kind>/`: the
settings list in order, then the repository's `.e/packages` list, then
`--package` packages.

After you trust a repository, e also reads its own `.e/skills/` and
`.e/prompts/`. Extensions and themes never load from a repository, because
trusting a checkout must not run its code or restyle your terminal; install
the repository as a package instead.

On a name clash, the closer context wins: a repository skill shadows a
global one, and a global one shadows a package's. A package theme can reuse
a built-in name such as `dark` and replace it, unless
`~/.e/themes/dark.json` exists.

| Kind | Where it appears |
| --- | --- |
| Extensions | Launch like any in `~/.e/extensions/`; see [extensions](extensions.md). Their settings live under their own name in `"extensions"` in `settings.json`. |
| Skills | Show `Package` as their scope in the `$` picker. |
| Prompts | Become `/name` commands. |
| Themes | Appear under Theme in `/settings`. |

## Release packages

A compiled extension installs from a GitHub release:

```sh
e install release:<owner>/<repo>/<name>        # the latest release
e install release:<owner>/<repo>/<name>@v1.2.0 # pinned
```

e downloads `<name>-<target>.tar.gz` for this machine from the release,
verifies it against the release's `checksums.txt`, and installs the
`<name>` executable as `extensions/<name>` in the package directory. The
supported targets are `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`aarch64-unknown-linux-gnu`, and `x86_64-unknown-linux-gnu`; other platforms
cannot install release packages.

To publish one:

1. Build one asset per target, named `<name>-<target>.tar.gz`, with the
   executable at the top level of the archive.
2. List every asset in the release's `checksums.txt`, in `sha256sum` format.

## Publish a package

Create a package, try it in place, then publish it:

```sh
e packages init my-package     # the four directories, one extension, package.json, README
cd my-package && e --package . # try it in place
npm publish                    # then: e install npm:my-package
```

`e packages init` refuses a directory that is not empty. The `package.json`
it writes carries the `e-package` keyword, so people can find the package
by searching npm for that keyword, and a `files` list naming the four
directories and the README, so nothing else ships.

Keep extensions executable (`chmod +x`) and commit the mode; npm and git
both carry it. Tag or version your releases so users can pin what they
read. A git repository with the same layout installs as
`git:<host>/<user>/<repo>` without npm.

A package can contain one extension file and one prompt template, installed
with a single `e install`. Commands such as `/diff` can live in an extension
package; e does not need to build them in.
