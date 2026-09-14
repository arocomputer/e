# Working on e

Instructions for an agent editing this repo.

## Build and check

```sh
cargo build          # fast dev build
./x test             # the whole behavioral contract
./x check            # format, lint, tests, and security-surface guard
./x bench            # release-mode performance budgets
./x ui               # PTY frame/color checks; makes its own Python env on first run
```

`./x test` is not optional. The visual design is pinned byte-for-byte in
`tests/` against the reference design's own literals. If a rendering change
makes the tests fail, you drifted the look — fix the code, don't loosen the
test.

## Where things live

```
src/core/    the harness, terminal-free
  agent/          mod.rs owns run lifecycle and session state;
                  turn.rs: request → stream → tools → compact when needed → repeat;
                  compact.rs (threshold, protected instructions, summarize)
                  · context.rs (system prompt, AGENTS.md, skills catalog)
  providers/      the seam (mod.rs) — one Request, one Event stream, the SSE
                  splitter · api/{completions,responses,anthropic,google}.rs ·
                  registry.rs + data/*.json (providers are data: gateway,
                  dialect, auth surface, seed models) · catalog/ (assembly,
                  availability, scope; remote.rs = the live /models sync)
  auth/           credentials (mod.rs) · login.rs (OAuth, device-code, API keys)
  config/         the ~/.e surface: home.rs (paths) · store.rs (merge-write)
                  · settings.rs · trust.rs (per-directory trust) ·
                  keybindings.rs (composer chord overrides)
  resources/      skills.rs · prompts.rs (/name templates) · packages.rs
                  (`e install`: git clones under ~/.e/packages that every
                  loader reads after ~/.e's own dirs) · docs.rs (the
                  embedded guides behind `e docs`)
  extensions/     the extension host: subprocesses over a JSONL line
                  protocol (docs/extensions.md) — tools, commands, hooks,
                  events, and the extensions' own ui.*/session.* requests
                  (HostRequest, answered by the frontend; decision 0005)
  tools/          read · write · edit · grep (optional `glob` filter) · bash
                  (optional `background`/`handle` for long-lived processes) ·
                  read_result (page into a truncated result by id; the
                  runtime keeps the whole text) — the whole surface;
                  directory listing and file-finding go through bash, and
                  skills load through read (the catalog carries their paths)
  session.rs · output.rs · workspace.rs — SessionLog is a tree, not just a
                  line: id/parent per message, `/tree` branches in place,
                  `create_with` seeds a `/fork`; `responses_in` feeds usage.rs
                  (the /usage fold) · export.rs (a branch as one HTML page)
src/tui/     the frontend (short paths re-export from the groups)
  paint/          render · screen · theme · background · highlight
  content/        markdown · transcript · composer · statusline · history
                  (prompts across sessions, ~/.e/history.jsonl)
  surfaces/       panel · menu · settingspanel · authpanel · trustpanel
  app/            mod.rs (App state, keys, the frame loop) · events.rs
                  (session-event handling) · menus.rs (footer menus) ·
                  login.rs (sign-in flows) · extui.rs (answering
                  extensions: modals, panels, status slots, session control)
src/main.rs  CLI entry — flags, rpc/docs/auth/update, then tui::app::run
sdk/         e-sdk, the in-process Rust surface (docs/sdk.md): session.rs
             (builder, Session) · turn.rs (Turn, Event, Reply) · error.rs;
             a consumer of the library target with its own release boundary
             (decisions/0002), never a fourth layer
```

## Running one thing, not everything

Each `tests/*.rs` file is its own binary; the fast loops are:

```sh
cargo test --test stream            # agent turn loop against a mock provider
cargo test --test providers         # the four wire dialects' request/stream shapes
cargo test --test parity            # byte-pinned rendering (run after any look change)
cargo test --test toolloop          # end-to-end tool execution
cargo test name_of_one_test         # any single test, by name substring
```

New integration tests use `tests/common/` (`mod common;`) — `Home` for an
isolated `E_HOME`, `env_lock()` around anything env-global, `serve_sse` +
`test_model` for a mock provider. Don't hand-roll a second mock harness.

## How the look stays consistent

Every colour comes from the theme (`theme.fg("token", text)`), never a raw SGR
literal — the palette is the single source of truth, and it is the reference
design's, audited value-for-value. Panel dividers (`tui/surfaces/panel.rs`)
use the `border` token (divider_style, 240/250), not `dim` — markdown's own
thematic-break rule and blockquote rail are a different, dimmer reference
element and are pinned as `dim` in `tests/parity.rs`; don't conflate the two.
Selection is brightness alone on every picker — bold bright ink for the
current row (models, skills, sessions, /tree, and the inline `/`/`@`/`$`
completion pickers all the same), unselected rows stay `dim`, and no picker
fills the row. No caret either way (the auth panel's `› ` is its own reference
element). The `/` picker's rows carry a right-aligned category that brightens
with the selected row: a built-in shows its functional group (`Account`,
`Model`, `Session`, `Workspace`, `General` — see `builtin_category`), a prompt
template reads `Prompt`, an extension command `Extension`.

Every footer surface (the `/@$` pickers, `/settings`) frames through
`tui/surfaces/panel.rs`: top divider, header, blank, body, bottom divider, with
the hint on the status row — never a second hint inside a panel. Add a new
surface? Route it through `panel.rs` so it can't diverge.

## Conventions

- One event stream. The frontend subscribes once; text, tools, usage, errors all
  arrive on it in order (`SessionEvent`). Compaction and continuation belong
  to the core. Frontends never reset running state or resubmit stranded prompts.
- Hit every consumer. The core has three frontends — the TUI, `e rpc`, and
  `sdk/` — and four provider dialects. A change to the turn loop, events, or
  tools needs a decision per frontend, and a provider-shaped change a decision
  per dialect, even when the decision is "no change here". Persisted and
  user-facing contracts (CLI, sessions, configuration, the extension protocol)
  follow `docs/compatibility.md`: fixtures under `tests/fixtures/` are release
  artifacts, so a contract change adds or updates one in the same PR.
- Keep the harness small. Prefer a spawned process over a daemon and a gate
  over a pipeline. Add complexity only when the feature requires it.
- `~/.e/` is the only home e reads. Never reach into another tool's directory.
- A package is a directory shaped like `~/.e/` (`extensions/ skills/ prompts/
  themes/`), no manifest. New resource kinds join that list; package
  discovery stays convention, not configuration.
- **Don't hardcode what a user might change.** Looks, wordings, and behaviours a
  person could sensibly prefer are read from `~/.e/` with a built-in default —
  themes from `~/.e/themes/`, and skills, prompts, instructions, the system
  prompt the same way. When you add something user-facing, make it a file-backed
  override, not a constant. When data isn't enough there is the extension API
  (`core/extensions/`, docs/extensions.md) — grow its protocol by need, never by
  symmetry, and keep hooks fail-open. What crosses the line is data, never code
  or terminal bytes: an extension describes (`show`, `panel`, a `label`), e
  paints through the theme. A new rendering need is a new `format` or token, not
  a way for extensions to emit escape sequences.
- Verify UI changes with a real frame, not by reasoning about bytes. `./x ui`
  runs checked PTY scenarios under `tests/ui/`, sharing the capture/replay
  helpers in `scripts/`. See `tests/ui/README.md` for setup and retained frames.
- `scripts/guard.sh` pins the trust boundary: allowed network hosts, the
  sovereign home, store-only config writes, where `unsafe` lives, SHA-pinned
  CI actions. If a change legitimately moves a boundary, update the guard in
  the same commit — never work around it.
- Docs: a choice that is expensive to reverse (a persisted or wire format, a
  trust boundary, process architecture, a cross-cutting invariant) gets a
  `docs/decisions/` entry. Everything else is a comment beside the code, or
  nothing — never a PR summary appended to a doc. When a change makes existing
  guidance wrong, rewrite it; don't add a second account next to the first.

## Pull requests

- Never open a PR unless the developer explicitly asks you to.
- Conventional commit titles, plain language: `fix(tui): tool trees stay
  connected after compaction`. The type is `fix`, `feat`, `perf`, `refactor`,
  `docs`, `test`, or `chore`; the scope is the area triage labels by path —
  `core`, `tui`, `sdk`, `bench`, `infra`, `docs` — or omitted when the change spans
  them. The title becomes the squash commit on `main`, so write it as the
  one line someone reads in `git log`.
- Body: the problem in a sentence or two, then how you fixed it. End with
  the model and harness that did the work.
- Rendering changes carry a captured frame (`scripts/ptycap.py`), not a
  description of bytes.
- One concern per PR. If the description says "also", split it. Unrelated
  cleanup you spotted mid-change is its own PR.
- Behavior that changes gets a test, and a regression test fails against the
  unfixed code for the intended reason. Anything user-visible gets a
  `CHANGELOG.md` entry under `Unreleased`; CI, templates, and result files
  don't.
