# Packages

A package bundles extensions, skills, prompt templates, and themes so they
can be shared. It is a git repository (or a directory on disk) laid out like
`~/.e/` itself — any subset of these four directories, nothing else required:

```
extensions/   executables, or bundle directories with an entry point
skills/       <name>/SKILL.md folders
prompts/      <name>.md templates
themes/       <name>.json palettes
```

There is no manifest. The repository's own README, description, and tags
describe it; e reads the directories.

> **Security:** a package runs with your full permissions. Its extensions are
> executables e starts at launch, and its skills and prompts steer the model.
> Read the source before installing anything, and pin a ref you have read.

## Install and manage

```sh
e install git:github.com/fschrhunt/e-diff       # follow the default branch
e install git:github.com/fschrhunt/e-diff@v2    # pin a tag, branch, or commit
e install https://github.com/user/repo          # any git URL works
e install git:git@github.com:user/repo@main     # SSH, with your keys
e install ~/src/my-package                      # a local directory, in place

e packages                                      # what is listed, and its state
e remove git:github.com/fschrhunt/e-diff        # forget it, delete the clone
e install                                       # make disk match settings
```

Installing clones the repository under `~/.e/packages/<host>/<path>` and
records the source in the `packages` list of `~/.e/settings.json`. Restart e
or run `/reload` to pick it up.

Settings are the source of truth, not the directory:

- `e install` with no source clones every listed package that is missing
  and brings every git package current: a pinned ref is checked out again,
  an unpinned one fast-forwards to its default branch. Copy `settings.json`
  to a new machine and one `e install` restores the set.
- `e install <same package>@<other ref>` moves the pin — one entry, not two.
  Identity ignores scheme, credentials, `.git`, and host case, so the HTTPS
  and SSH spellings of one repository are the same package.
- A listed package that is not on disk is reported once in the transcript at
  startup. Startup itself never touches the network; only `e install` does.
- A local directory is referenced where it is, never copied. `e remove` only
  forgets it.

Git runs as a subprocess, so whatever your `git` can clone, e can install —
private hosts, SSH config, credential helpers included. Set
`GIT_TERMINAL_PROMPT=0` in automation to fail fast instead of prompting.

## How package resources load

Every loader reads `~/.e/<kind>/` first, then each installed package's
`<kind>/` in settings order, then — after `/trust` — the repository's own
`.e/<kind>/`. On a name clash the closer context wins: a repo skill shadows a
global one, and a global one shadows a package's. A package theme can name a
built-in (`dark`) and replace it, unless `~/.e/themes/dark.json` exists.

- **Extensions** launch like any in `~/.e/extensions/`; their config lives
  under their own name in `settings.json` → `"extensions"`.
- **Skills** show `Package` as their scope in the `$` picker.
- **Prompts** become `/name` commands.
- **Themes** appear in `/settings` → Theme.

## Release packages

A compiled extension installs from a GitHub release:

```sh
e install release:<owner>/<repo>/<name>        # the latest release
e install release:<owner>/<repo>/<name>@v1.2.0 # pinned
```

e downloads `<name>-<target>.tar.gz` for this machine's platform from the
release, checks it against the release's `checksums.txt`, and places the
`<name>` executable under `~/.e/packages/releases/<owner>/<repo>/<name>/extensions/`,
the same shape as every other package. An unpinned release package follows
the latest release on `e install`; `e remove` deletes it. Platforms are the
ones e itself is released for.

To publish one, name the asset `<name>-<target>.tar.gz` with the executable
at its top level, list it in the release's `checksums.txt` (`sha256sum`), and
build one per target e is released for (`e update` names them). A Rust
extension may depend on the `e-terminal` crate from the e repository for
the host's palette and text primitives; the e repository itself ships no
extensions, so nothing about a package needs a change to e.

## Publishing a package

1. Create a repository with the directories above. Include a README that
   says what each resource does and a LICENSE.
2. Tag releases (`v1`, `v1.1`) so users can pin what they read.
3. Add the `e-package` topic on GitHub so others can find it:

   ```sh
   gh search repos --topic e-package
   ```

[fschrhunt/e-diff](https://github.com/fschrhunt/e-diff) shows the shape: one
extension file and one prompt template, installable with a single `e
install`. It is how `/diff` reaches e — a package, not a part of it.

Try a package before publishing it with a local install:

```sh
e install ./my-package && e
```
