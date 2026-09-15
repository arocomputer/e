# Contributing

e is a small, fast coding agent for the terminal: one Rust binary, no daemon,
no plugin runtime beyond executable JSONL extensions. That smallness is the
point, and it shapes what a good contribution looks like. The best ones solve
one clear problem, add the least code that solves it, and leave the repository
easy to verify. If a change needs an abstraction layer, a config option, or a
new protocol message to land, say why in the PR; if it doesn't, don't add one.

## Getting set up

```sh
git clone https://github.com/intuitums/e
cd e
cargo build
./x test
```

The Rust toolchain is pinned in `rust-toolchain.toml`; rustup installs the
right version on its own. A full build plus test run is the fastest way to
find out whether your machine is set up correctly.

## Reporting issues

Use the bug or feature issue form. Bug reports need a reproducible case and
the version (`e --version`, or the commit if you built from source). Feature
requests should explain the need before the design; an implementation sketch
is welcome but optional. A feature that could be an extension is usually
better as one — the extension API in [docs/extensions.md](docs/extensions.md)
exists precisely so most additions never have to touch the binary, and
[docs/packages.md](docs/packages.md) is how an extension, skill, prompt, or
theme reaches other users without a release of e.

## Finding your way around

[docs/architecture.md](docs/architecture.md) is the guided tour.
[AGENTS.md](AGENTS.md) is the working guide: the code map, the fast test
loops, how the look stays consistent, and the conventions every change
follows. It is written for the agents that open most PRs here, and it is the
same set of rules for you. Read it before your first change; nothing in it is
repeated here.

## Before you open a PR

```sh
./x check    # format, clippy, full test suite, security-surface guard
./x bench    # release-mode performance budgets
```

`./x` is the single definition of green; CI runs the same commands, so
nothing merges on a private definition of passing.

## Review

Every change needs the maintainer's review. Paths that form the trust
boundary — the extension host, authentication, the config store, provider
wire code, session persistence, `install.sh`, and `.github/` — are called out
in [CODEOWNERS](.github/CODEOWNERS) and cannot merge on green checks alone.

Title the PR as a conventional commit in plain language, scoped by area:
`fix(tui): tool trees stay connected after compaction`. Scopes are the ones
triage labels by path — `core`, `tui`, `sdk`, `bench`, `infra`, `docs` — and the
title becomes the squash commit on `main`. In the body, state the problem in
a sentence or two, then how you fixed it; the template only carries the
checklist CI and review expect. One concern per PR — if the description
says "also", split it. When a change alters a persisted or wire contract
(`docs/compatibility.md`), add the `breaking` label yourself: the triage
workflow can label paths, but no path tells it a contract changed.

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
