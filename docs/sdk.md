# SDK

The `e-sdk` package (`sdk/`) is e's coding agent as a Rust library: create a
session against a working directory, prompt it, read one ordered event
stream, get a reply. It links the same core the terminal frontend drives —
the built-in tools, skills and AGENTS.md context, automatic compaction,
on-disk session logs, extensions — without a terminal.

Until the first release declares its semantic-versioning policy, the
surface is unstable (see [compatibility.md](compatibility.md)).

## Why a separate package

The SDK is not part of `core/` and not an extension. It is a second in-repo
consumer of e's library target, given its own release boundary so that
stabilizing an API is a deliberate act rather than an accident of
visibility. See [decisions/0002](decisions/0002-rust-sdk-package.md).

## Building

```sh
cargo build -p e-sdk
cargo test -p e-sdk
cargo run -p e-sdk --example ask -- "what does this repository do"
```

The package is a member of the root workspace, so `./x check` and `./x test`
cover it like the rest of the repository. It needs a Tokio runtime: the core
spawns its turn worker and runs tool I/O on the blocking pool.

## Shape

```rust
use e_sdk::{Event, Session};

let mut session = Session::builder()
    .cwd("/path/to/project")
    .model("anthropic/claude-opus-4.7")
    .build()
    .await?;

let mut turn = session.prompt("What does this repository do?");
while let Some(event) = turn.next().await {
    match event {
        Event::Text(delta) => print!("{delta}"),
        Event::ToolCall { name, arguments, .. } => eprintln!("→ {name} {arguments}"),
        _ => {}
    }
}
let reply = turn.finish().await?;
println!("{} output tokens", reply.usage.output);

// Just the reply, no events:
let reply = session.prompt("And the tests?").await?;

session.close().await;
```

Three types carry the design:

- **`Session`** — one conversation. Built once from a `SessionBuilder`
  (working directory, home, model, effort, tools, persistence, extensions,
  host instructions, resume or seed history); prompted many times. Between
  turns: `history()`, `set_model()`, `clear()`, `path()`.
- **`Turn`** — one prompt's run. Lazy: nothing is sent until it is first
  polled. Iterate it for `Event`s, then `finish()` for the `Reply`; or
  `.await` it directly to skip the events. `steer()` adds a message to the
  running turn, delivered before its next provider request. `cancel()` is
  Esc. Dropping a running turn also interrupts it.
- **`Reply`** — what the turn produced: the joined assistant text, token
  usage, an optional cost estimate, tool counts, whether it completed or was
  cancelled, and any warnings.

Everything that can be checked up front fails in `build()`, with an
`Error` naming what is wrong (unavailable model, no signed-in provider,
unsupported effort, unknown tool, locked session file). A turn that ran and
failed returns a `TurnError` whose `reply` holds everything the turn
produced before it failed.

## Rules the types enforce

- **One turn at a time.** `prompt` borrows the session mutably for the
  turn's lifetime. A second prompt cannot start until the first has finished
  or been dropped; mid-turn input goes through `Turn::steer`.
- **Nothing is lost.** The turn reads the core's event channel directly, so
  an unread event holds the model instead of growing a buffer; a failed
  turn's partial text, usage, and tool counts come back inside the error.
- **Nothing touches `~/.e` unless asked.** Conversations are memory-only
  unless `persist(true)`; extensions start only with `extensions(true)`;
  the SDK never writes settings. Effort set at build is process-local.
- **Configuration is injected, not inherited.** `home()` scopes every
  configuration read to that directory without touching the process
  environment, so sessions with different homes coexist in one process.
  Without it, the home is `E_HOME`, then `~/.e`, as for the terminal.

## Events

In the order they happen: `Text` and `Reasoning` deltas; `ToolCall` (the
model asked, with the raw JSON arguments — every call in one assistant
message is announced before any runs), `ToolStart`, `ToolOutput` (live
command output, a preview), `ToolEnd` (outcome and the retained content);
`Usage` per provider request; `Compacting` and `Compacted` when the context
window is checkpointed mid-turn; `Retry` with the backoff; `Steered`,
`Discarded`; `Named` when an extension names the session; `Warning`; and
`Notice` for extension messages. The core's error is not an event — it ends
the turn, so it arrives as the `TurnError`.

## Sessions on disk

`persist(true)` writes the conversation to a JSONL log under the home's
`sessions/`, the same files `e -r` lists, in the documented session format.
`SessionBuilder::saved()` lists a workspace's logs; `resume(path)` continues
one in place, holding its lock; `e_sdk::transcript(path)` reads one without
taking ownership, and `history(messages)` seeds a memory-only session from
it. That is the whole checkpoint story: a readable file, not opaque bytes.

## What the SDK is not

- **Not an extension.** Extensions are child processes speaking a JSONL
  protocol to a running e ([extensions.md](extensions.md)). The SDK links
  the core into your program, and can start the home's extensions for their
  tools and hooks (startup hooks and extension flags are CLI concerns and
  do not run).
- **Not a daemon.** e stays a spawned process; there is no server to run.
- **Not a tool kernel.** The SDK runs e's tools in the working directory you
  give it, as your user, without a permission prompt — the same safety
  contract as the terminal. Host-defined in-process tools are not part of
  the surface; use an extension.

If you are integrating from another language, the JSON output and RPC modes
in [automation.md](automation.md) remain the language-agnostic surface; the
SDK is the in-process Rust alternative.
