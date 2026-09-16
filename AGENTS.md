# Working on e

Instructions for an agent editing this repo.

## Build and check

```sh
./x hooks            # required once per contributing worktree
cargo build          # fast dev build
./x test             # the whole behavioral contract
./x check            # format, lint, tests, and security-surface guard
./x bench            # release-mode performance budgets
./x ui               # PTY frame/color checks; makes its own Python env on first run
./x site             # build the docs site in docs/; needs Node
```

`./x test` is not optional. The visual design is pinned byte-for-byte in
`crates/cli/tests/` against the reference design's own literals. If a rendering change
makes the tests fail, you drifted the look — fix the code, don't loosen the
test.

## Where things live

Every Rust crate is a folder under `crates/`. tui, rpc, and sdk depend on core
and never on each other; cli puts the frontends behind one binary.

```
crates/core/ the harness, terminal-free (`e_core`)
  build.rs        stamps release identity and embeds the guides (`guides` is
                  a link to docs/guides) · themes/ (the built-in palettes)
  agent/          mod.rs owns run lifecycle and session state;
                  event.rs is the frontend contract; persistence.rs commits
                  history and logs and reports write failures;
                  turn.rs: request → stream → tools → compact when needed → repeat;
                  compact.rs (threshold, protected instructions, summarize)
                  · context.rs (system prompt, AGENTS.md, skills catalog)
  providers/      mod.rs (Request and Event), message.rs (persisted messages),
                  error.rs (failure classification), transport.rs (HTTP deadlines),
                  sse.rs (bounded framing) · api/{completions,responses,anthropic,google}.rs ·
                  registry.rs + data/*.json (providers are data: gateway,
                  dialect, auth surface, seed models) · catalog/ (assembly,
                  availability, scope; remote.rs = the live /models sync;
                  modelsdev.rs = model facts from models.dev)
  auth/           credentials (mod.rs) · login.rs (OAuth, device-code, API keys)
  config/         the ~/.e surface: home.rs (paths) · store.rs (merge-write)
                  · settings.rs · trust.rs (per-directory trust) ·
                  chord.rs (the chord grammar keybindings, layout focus,
                  and extension shortcuts share). Terminal-free: core
                  depends on no frontend crate and no terminal library —
                  guard.sh pins the manifests
  resources/      skills.rs · prompts.rs (/name templates) · packages.rs
                  with packages/source.rs (source parsing); `e install` manages
                  npm, git, local, and release bundles that every
                  loader reads after the active home's own dirs · docs.rs (the
                  embedded guides behind `e docs`)
  extensions/     the extension host: host.rs owns process lifecycle and routing;
                  host/discovery.rs finds entry points, host/hooks.rs handles
                  hooks and events, host/transport.rs owns bounded JSONL I/O.
                  Subprocesses use a JSONL line
                  protocol (docs/guides/extend/extensions.md) — tools, commands, hooks,
                  events, and the extensions' own ui.*/session.* requests
                  (HostRequest, answered by the frontend)
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
crates/tui/  the terminal frontend (`e_tui`; short paths re-export from the groups)
  paint/          render · screen · theme · background · highlight
  content/        markdown · transcript · composer · keybindings (the
                  ~/.e/keybindings.json keymap) · statusline · history
                  (prompts across sessions, ~/.e/history.jsonl)
  surfaces/       panel · menu · settingspanel · authpanel · trustpanel
  app/            mod.rs (App state and shared actions) · runtime.rs (startup
                  and frame loop) · frame.rs (painting) · input.rs (composer
                  and attachments) · sessions.rs (navigation) · events.rs
                  (session-event handling) · menus.rs (footer menus) ·
                  login.rs (sign-in flows) · extui.rs (answering
                  extensions: modals, panels, status slots, session control)
crates/rpc/  the headless frontend (`e_rpc`): `e rpc`, a JSONL session server over
             stdin/stdout (docs/guides/usage/automation.md) — lib.rs (sessions, methods,
             the serve loop, extension questions relayed as `ask`) ·
             params.rs (typed method inputs) · result.rs (the turn result
             `-p --json` and rpc both report)
docs/        guides/: the guides, one folder per nav group, with front matter
             as their only metadata (docs/README.md is the writing guide);
             `e docs` embeds them, and the Starlight site beside them
             (astro.config.mjs, src/) renders them at e.intuitum.sh/docs. contributing/ is the
             repository's own documentation, never published.
crates/cli/  the `e` binary, published as intuitums-e: src/main.rs (flags,
             rpc/docs/auth/update, then tui::app::run) · src/lib.rs (the `e`
             library, re-exporting core, tui, and rpc for the binary, tests,
             and fuzz targets) · tests/ (the integration suites, fixtures,
             and the ui/ PTY scenarios)
crates/sdk/  e-sdk, the in-process Rust surface (docs/guides/extend/sdk.md): session.rs
             (builder, Session) · turn.rs (Turn, Event, Reply) · error.rs;
             a frontend over core alone, with its own release boundary
channels/    reference clients of `e rpc` (docs/guides/usage/channels.md): slack/ (a Bolt
             bot, TypeScript) · github/ (an Actions workflow). Not compiled
             into e; a channel is a program that spawns it, never a module
```

## Running one thing, not everything

Each `crates/cli/tests/*.rs` file is its own binary; the fast loops are:

```sh
cargo test --test stream            # agent turn loop against a mock provider
cargo test --test providers         # the four wire dialects' request/stream shapes
cargo test --test parity            # byte-pinned rendering (run after any look change)
cargo test --test file_tools        # read/write/edit/grep behavior
cargo test -p intuitums-e-sdk        # embedded SDK consumer
cargo test --test toolloop          # end-to-end tool execution
cargo test name_of_one_test         # any single test, by name substring
```

New integration tests use `crates/cli/tests/common/` (`mod common;`) — `Home` for an
isolated `E_HOME` that restores the prior value on drop, `env_lock()` around anything env-global, `serve_sse` +
`test_model` for a mock provider. Don't hand-roll a second mock harness.
Use explicit `AgentOptions` home/cwd paths for concurrent embedded sessions;
process environment belongs only in serialized fixtures. Keep specialized
servers when the test needs transport timing or malformed bytes.

`./x check` covers all workspace members and an external consumer compiled from
the packed SDK and core crates. It does not replace `./x packages`, `./x channels`,
or `./x ui`; run the relevant command when changing those paths.

## How the look stays consistent

Every colour comes from the theme (`theme.fg("token", text)`), never a raw SGR
literal — the palette is the single source of truth, and it is the reference
design's, audited value-for-value. Panel dividers (`crates/tui/src/surfaces/panel.rs`)
use the `border` token (divider_style, 240/250), not `dim` — markdown's own
thematic-break rule and blockquote rail are a different, dimmer reference
element and are pinned as `dim` in `crates/cli/tests/parity.rs`; don't conflate the two.
Selection is brightness alone on every picker — bold bright ink for the
current row (models, skills, sessions, /tree, and the inline `/`/`@`/`$`
completion pickers all the same), unselected rows stay `dim`, and no picker
fills the row. No caret either way (the auth panel's `› ` is its own reference
element). The `/` picker's rows carry a right-aligned category that brightens
with the selected row: a built-in shows its functional group (`Account`,
`Model`, `Session`, `Workspace`, `General` — see `builtin_category`), a prompt
template reads `Prompt`, an extension command `Extension`.

Every footer surface (the `/@$` pickers, `/settings`) frames through
`crates/tui/src/surfaces/panel.rs`: top divider, header, blank, body, bottom divider, with
the hint on the status row — never a second hint inside a panel. Add a new
surface? Route it through `panel.rs` so it can't diverge.

## Conventions

- One event stream. The frontend subscribes once; text, tools, usage, errors all
  arrive on it in order (`SessionEvent`). Compaction and continuation belong
  to the core. Frontends never reset running state or resubmit stranded prompts.
- Hit every consumer. The core has three frontends — the TUI, `e rpc`
  (`crates/rpc/`), and `crates/sdk/` — and four provider dialects. A change to the turn loop, events, or
  tools needs a decision per frontend, and a provider-shaped change a decision
  per dialect, even when the decision is "no change here". Persisted and
  user-facing contracts (CLI, sessions, configuration, the extension protocol)
  follow `docs/guides/extend/compatibility.md`: fixtures under `crates/cli/tests/fixtures/` are release
  artifacts, so a contract change adds or updates one in the same PR, and
  labels it `breaking` — the one label no path can apply for you.
- Keep the harness small. Prefer a spawned process over a daemon and a gate
  over a pipeline. Add complexity only when the feature requires it.
- Resolve the active home through `crates/core/src/config/home.rs`: stable uses `~/.e/`,
  previews use their channel home, and `E_HOME` overrides either. Never read
  another tool's directory.
- A package is a directory shaped like `~/.e/` (`extensions/ skills/ prompts/
  themes/`), no manifest. New resource kinds join that list; package
  discovery stays convention, not configuration.
- **Don't hardcode what a user might change.** Looks, wordings, and behaviours a
  person could sensibly prefer are read from `~/.e/` with a built-in default —
  themes from `~/.e/themes/`, and skills, prompts, instructions, the system
  prompt the same way. When you add something user-facing, make it a file-backed
  override, not a constant. When data isn't enough there is the extension API
  (`crates/core/src/extensions/`, docs/guides/extend/extensions.md) — grow its protocol by need, never by
  symmetry, and keep hooks fail-open. What crosses the line is data, never code
  or terminal bytes: an extension describes (`show`, `panel`, a `label`), e
  paints through the theme. A new rendering need is a new `format` or token, not
  a way for extensions to emit escape sequences.
- Verify UI changes with a real frame, not by reasoning about bytes. `./x ui`
  runs checked PTY scenarios under `crates/cli/tests/ui/`, sharing the capture/replay
  helpers in `scripts/`. See `crates/cli/tests/ui/README.md` for setup and retained frames.
- `scripts/guard.sh` pins the trust boundary: allowed network hosts, the
  sovereign home, store-only config writes, where `unsafe` lives, SHA-pinned
  CI actions. If a change legitimately moves a boundary, update the guard in
  the same commit — never work around it.
- Docs: keep current contracts in the relevant guide and implementation details
  beside the code. When guidance becomes wrong, rewrite it rather than appending
  another account. Use Git history for past decisions.

## Branches and pull requests

- Before the first push, name the branch `<type>/<slug>`. Use the PR title's
  conventional type or scope plus two or three lowercase words, for example
  `bench/real-launches` or `fix/tool-tree-compaction`. Never push `main`, a bare
  SHA, or a vague generated name.
- Never open a PR unless the developer explicitly asks you to.
- Conventional commit titles, plain language: `fix(tui): tool trees stay
  connected after compaction`. The type is `fix`, `feat`, `perf`, `refactor`,
  `docs`, `test`, or `chore`; the scope is the area triage labels by path —
  `core`, `tui`, `sdk`, `bench`, `infra`, `docs` — or omitted when the change spans
  them. The title becomes the squash commit on `main`, so write it as the
  one line someone reads in `git log`.
- Body: the problem in a sentence or two, then how you fixed it. Never
  attribute work to AI: no `Co-authored-by`, `Assisted-by`, or similar
  trailer, no model or harness line, no agent self-mention. The AI/LLM
  rules live in CONTRIBUTING.md.
- Rendering changes carry a captured frame (`scripts/ptycap.py`), not a
  description of bytes.
- One concern per PR. If the description says "also", split it. Unrelated
  cleanup you spotted mid-change is its own PR.
- Behavior that changes gets a test, and a regression test fails against the
  unfixed code for the intended reason. Anything user-visible gets a
  `CHANGELOG.md` entry under `Unreleased`; CI, templates, and result files
  don't.
