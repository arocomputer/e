# Contributing

e is the coding agent you can put anywhere: one Rust binary, no daemon,
no plugin runtime beyond executable JSONL extensions. That smallness is the
point, and it shapes what a good contribution looks like. The best ones solve
one clear problem, add the least code that solves it, and leave the repository
easy to verify. If a change needs an abstraction layer, a config option, or a
new protocol message to land, say why in the PR; if it doesn't, don't add one.

## Getting set up

```sh
git clone https://github.com/arocomputer/e
cd e
./x hooks
cargo build
./x test
```

The Rust toolchain is pinned in `rust-toolchain.toml`; rustup installs the
right version on its own. A full build plus test run is the fastest way to
find out whether your machine is set up correctly.

### Commit checks

Run `./x hooks` in each contributing checkout or worktree. This is part of the
required setup. It enables the repository pre-commit hook for that worktree and
preserves an existing executable pre-commit hook. Setup refuses to replace other
active hooks; integrate those explicitly before enabling it.

Before each commit, the hook checks staged changes for whitespace errors and
conflict markers, then checks formatting for changed Rust files using their
staged content. It never rewrites files or stages changes. Full tests and builds
remain separate so commits stay fast. Python 3 and the pinned Rust toolchain are
required.

Git allows local hooks to be bypassed. The required CI Guard check runs the same
content checks on every PR, including docs-only changes, so bypassing a hook does
not bypass merge checks.

## Reporting issues

Use the bug or feature issue form. Bug reports need a reproducible case and
the version (`e --version`, or the commit if you built from source). Feature
requests should explain the need before the design; an implementation sketch
is welcome but optional. A feature that could be an extension is usually
better as one — the extension API in [docs/guides/extend/extensions.md](docs/guides/extend/extensions.md)
exists precisely so most additions never have to touch the binary, and
[docs/guides/extend/packages.md](docs/guides/extend/packages.md) is how an extension, skill, prompt, or
theme reaches other users without a release of e.

## Finding your way around

[contributing/architecture.md](contributing/architecture.md) is the guided tour.
[AGENTS.md](AGENTS.md) is the working guide: the code map, the fast test
loops, how the look stays consistent, and the conventions every change
follows. It is written for the agents that open most PRs here, and it is the
same set of rules for you. Read it before your first change; nothing in it is
repeated here.

## Before you open a PR

```sh
./x check    # format, lint, full suite, crate packages, and guard
./x bench    # release-mode performance budgets
```

The `Tests` workflow runs lint, unit tests on Linux and macOS, terminal tests,
package checks, channels, documentation, glibc compatibility, and benchmarks.

GitHub prefixes each job with its workflow, for example `Tests / Terminal` or
`Security / Audit`. Required job names remain `changes`, `lint`, `unit (linux)`,
and `unit (macos)` so existing PR results continue to satisfy branch protection.
The dev publication workflow listens for a successful `Tests` run on main.

```sh
./x fmt --check   # formatting, fuzz targets included
./x lint          # clippy, warnings denied
./x test          # the suite; every failing binary reports, not just the first
./x crates        # the crates.io packages: packaged, built, and file-listed
./x docs          # the guides' contract: front matter, groups, links
./x guard         # the trust boundary and the repository's tooling tests
./x ui            # terminal frames and interaction scenarios
./x packages      # installers and package launchers
./x sbom /tmp/e-sbom.cdx.json  # application dependency inventory
```

The full suite includes the docs contract. Prose-only changes run that contract
and the site build without the full suite. Performance-related PRs run benchmarks;
main code changes and the weekly schedule run them too. Rust jobs cache
dependencies and build outputs by platform, job, toolchain, and dependency set.

`./x` is the single definition of green; CI runs the same commands, so
nothing merges on a private definition of passing. `scripts/guard.sh` enforces
the last part mechanically: the check workflow may not invoke `cargo` or `npm`
directly.

## Review

Every change needs the maintainer's review. Paths that form the trust
boundary — the extension host, authentication, the config store, provider
wire code, session persistence, `install.sh`, and `.github/` — are called out
in [CODEOWNERS](.github/CODEOWNERS) and cannot merge on green checks alone.

Title the PR as a conventional commit in plain language, scoped by area:
`fix(tui): tool trees stay connected after compaction`. Scopes are `core`,
`tui`, `sdk`, `bench`, `infra`, and `docs`, and the title becomes the squash
commit on `main`.

Use the [PR template](.github/pull_request_template.md). Link a related issue when
one exists, select the change type, explain the problem and why the change works,
and list verification commands and results. Include captured frames for visual
changes; remove that section when it does not apply. Write enough detail to review
the change without a fixed sentence limit.

Keep each PR about one coherent change and leave unrelated cleanup for another
contribution. When a change alters a persisted or wire contract
(`docs/guides/extend/compatibility.md`), explain the incompatibility and migration
in the description.

PRs do not use labels. Change types and breaking changes belong in the title and
description, not path-based or dependency labels. Issues can still use labels.
Template guidance is for review; automation does not label or close PRs for
template formatting.

## AI/LLM assistance

Creating issues and pull requests with AI/LLM help is fine, on one
condition: the content is yours to own.

- Review everything the AI produced — code, prose, commit messages —
  before you ask anyone here to review it for you.
- Never attribute a commit to AI/LLM as author, co-author, committer, or
  signatory: no `Assisted-by`, `Co-authored-by`, or similar trailer, and
  no generated footer naming the model or harness. Attribution here is
  human only.
- Answer maintainer questions and review comments yourself; what an
  agent wrote is input to your reply, not the reply.
- One AI-assisted pull request open at a time.

If you reach the point where you feel unwilling or unable to do the
above, close your issue or pull request.

## License

By contributing, you agree that your work is released under the repository's
[MIT license](LICENSE).

## Testing a working copy

Run `./x dev /path/to/project` to use the current checkout with development state.
Run `./x scenario streaming` for a repeatable local terminal session without a
provider account. See [releases and testing](contributing/releases.md) for beta channels,
PR builds, package installation, and release promotion.

Provider regressions can use reviewed response fixtures under
`crates/cli/tests/fixtures/providers/`. Existing seed fixtures are synthetic; their `origin`
field says so. `scripts/record-provider.py` records a real SSE response from an
explicit endpoint and request file. It sends a real request, potentially billable,
and never runs in CI. Use synthetic prompts, keep credentials in an environment
variable, and review response text for private content before committing it.
The recorder strips the supplied credential and named credential fields, but
cannot identify arbitrary private prose. Combine successive response bodies in
one fixture to exercise a tool loop through the existing `serve_sse` helper.
