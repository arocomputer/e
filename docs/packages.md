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
e install git:github.com/intuitums/diff       # follow the default branch
e install git:github.com/intuitums/diff@v2    # pin a tag, branch, or commit
e install https://github.com/user/repo        # any git URL works
e install git:git@github.com:user/repo@main   # SSH, with your keys
e install ~/src/my-package                    # a local directory, in place

e packages                                    # what is listed, and its state
e remove git:github.com/intuitums/diff        # forget it, delete the clone
e install                                     # make disk match settings
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

## First-party packages

e's own packages are Rust crates under `packages/` in the e repository
(`packages/diff` is the first; `packages/terminal` holds primitives they
share). They are workspace members, never compiled into the e binary, and
today they install by building and copying the executable into
`~/.e/extensions/` — see [diff.md](diff.md). A git package (above) and a
workspace crate are the same thing to e at runtime: an executable speaking
the line protocol. The two meet when releases ship the crates' binaries, at
which point `e install` can fetch them like any other package.

## Publishing a package

1. Create a repository with the directories above. Include a README that
   says what each resource does and a LICENSE.
2. Tag releases (`v1`, `v1.1`) so users can pin what they read.
3. Add the `e-package` topic on GitHub so others can find it:

   ```sh
   gh search repos --topic e-package
   ```

[intuitums/diff](https://github.com/intuitums/diff) shows the shape: one
extension file and one prompt template, installable with a single `e
install`. It is the git-package counterpart of `packages/diff`; the crate is
the one that carries e's own Git review.

Try a package before publishing it with a local install:

```sh
e install ./my-package && e
```
