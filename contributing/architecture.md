# Architecture

ulo is a Cargo workspace of five crates under `crates/`, in two directional
layers:

```text
crates/tui         crates/rpc                      crates/sdk
CLI / TUI          ulo rpc (JSONL session server)    Rust SDK
    │ subscribes to one ordered SessionEvent stream
    ▼
crates/core: terminal-free core
    ├── agent turn loop ──► provider wire dialects ──► model APIs
    ├── tools ────────────► the selected working directory
    ├── extension host ───► user-installed child processes over JSONL
    └── stores ───────────► the captured configuration home
```

`crates/cli` is the `ulo` binary. It parses the command line and starts the
terminal frontend or the session server.

Each crate boundary is a dependency rule that Cargo enforces. core depends
on no frontend and no terminal library, so the SDK embeds the agent without a
terminal. The frontends depend on core and never on each other.
`scripts/guard.sh` pins those manifests. A new crate needs one of four
things: an independent consumer, a release or API boundary, a platform
boundary, or a measured build-time benefit. File length alone is a reason to
extract a module, not a crate.

## Invariants

- The frontend receives text, tools, usage, warnings, and errors through one
  ordered event stream. There are no state side channels.
- The core owns run completion, steering, and compaction. `TurnEnd` means
  the run has stopped; a context checkpoint emits `Compacting` and
  `Compacted` without ending the run. RPC, the TUI, and the SDK share this lifecycle.
- Each agent captures its working directory and configuration home and owns
  file observations and background handles. Explicit `AgentOptions` paths
  allow concurrent callers without changing process environment variables.
- `SessionLog` stores the conversation tree. An OS-held sidecar lock excludes
  other writers. The sidecar stays on disk; ownership ends when the handle closes.
- `ChatMessage` carries a tagged `MessageKind`. Only assistant records hold
  tool calls; tool records require a call id. Existing JSONL formats still load.
- `core/` is terminal-free. Terminal behavior stays in `tui/`; the headless
  server stays in `rpc/`. Both are frontends of the same core, and a
  channel (a Slack bot, a CI job; docs/guides/usage/channels.md) is a client of `ulo rpc`,
  never a module of ulo.
- Provider differences terminate at the dialect seam; the agent loop consumes
  one request and event vocabulary.
- User-controlled behavior is file-backed or supplied by the extension
  process boundary. ulo does not embed a scripting runtime or daemon.
- `core/config/home.rs` resolves the active home. Stable uses `~/.ulo/`, preview
  channels use separate homes, and `ULO_HOME` overrides either. Agents capture that
  path at construction. Store writes merge unknown keys and replace files atomically.
- Resource packages use the same four resource directories as the home.
  Sources can be npm, git, local directories, or verified release archives.
  `packages/source.rs` parses identities; `packages.rs` owns installation and
  settings. Startup reads disk only; npm lifecycle scripts stay disabled.
- Trust gates whether ulo runs in a workspace at all, and with it the
  repository-provided context. It is not an execution sandbox.
  The complete threat model is in [../SECURITY.md](../SECURITY.md).

## Ownership when changing code

The code map in [AGENTS.md](../AGENTS.md) names the files. These boundaries decide
where new behavior belongs:

- Provider messages persist across releases. Keep their serialization independent
  of HTTP transport; errors classify retry behavior before the agent sees them.
- Agent persistence commits history and logs. Session events describe the result
  to all three frontends; frontends must not repair or restart core turns.
- RPC decodes optional parameters before applying defaults. Validate a proposed
  state change completely before committing it, including model and effort together.
- TUI modules share one `App`. Frames paint state, input submits work, session
  actions navigate history, and the runtime owns terminal cleanup. Keep asynchronous
  results tied to their session or draft generation.
- Channels own subprocesses and their own state files. They must bound shutdown
  and preserve unreadable state rather than treating it as a fresh installation.
