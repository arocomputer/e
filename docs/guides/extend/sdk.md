---
title: SDK
description: Embed e's agent core in your Rust programs.
order: 3
---

# SDK

The SDK is e's coding agent as a Rust library. It lives in `crates/sdk/` and is
published as `intuitums-e-sdk`.

With the SDK you create a session against a working directory, prompt it,
read the core's ordered event stream, and get a reply. Extension notices
fill the gaps in the event stream.

The SDK links the same core the terminal frontend drives, without a
terminal. You get the built-in tools, skills and `AGENTS.md` context,
automatic compaction, on-disk session logs, and extensions.

```sh
cargo add intuitums-e-sdk
```

## Quick start

This example opens a session, streams one turn's events, and then prompts
again for just the reply:

```rust
use e_sdk::{Event, Session};

let mut session = Session::builder()
    .cwd("/path/to/project")
    .model("anthropic/claude-opus-5")
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

The SDK needs a Tokio runtime. The core spawns its turn worker and runs tool
I/O on the blocking pool.

## Core types

Three types carry the design.

### `Session`

A `Session` is one conversation. You build it once from a `SessionBuilder`
and prompt it many times.

The builder sets the working directory, home, model, effort, tools,
persistence, extensions, host instructions, and resume or seed history.

Between turns, call `history()`, `set_model()`, `clear()`, or `path()`.

### `Turn`

A `Turn` is one prompt's run. It is lazy, so nothing is sent until you first
poll it.

- Iterate it for `Event`s, then call `finish()` for the `Reply`.
- Or `.await` it directly to skip the events.
- `steer()` adds a message to the running turn. e delivers it before the
  turn's next provider request.
- `cancel()` works like pressing Esc.
- Dropping a running turn also interrupts it.

### `Reply`

A `Reply` is what the turn produced. It holds:

- the joined assistant text
- token usage
- an optional cost estimate
- tool counts
- whether the turn completed or was cancelled
- any warnings

## Errors

`build()` fails on everything that can be checked up front. It returns an
`Error` that names what is wrong:

- unavailable model
- no signed-in provider
- unsupported effort
- unknown tool
- unusable working directory
- locked or unreadable session file

A turn that ran and failed returns a `TurnError`. Its `reply` holds
everything the turn produced before it failed. The core's error is not an
event. It ends the turn, so it arrives as the `TurnError`.

## Rules the types enforce

- **One turn at a time.** `prompt` borrows the session mutably for the
  turn's lifetime. A second prompt cannot start until the first has finished
  or been dropped. Send mid-turn input through `Turn::steer`.
- **Nothing is lost.** The turn reads the core's event channel directly, so
  an unread event holds the model instead of growing a buffer. A failed
  turn's partial text, usage, and tool counts come back inside the error.
- **Nothing touches `~/.e` unless you ask.** Conversations are memory-only
  unless you set `persist(true)`. Extensions start only with
  `extensions(true)`. The SDK never writes settings. Effort set at build is
  local to the process.
- **Configuration is injected, not inherited.** `home()` scopes every
  configuration read to that directory without touching the process
  environment. Sessions with different homes can coexist in one process.
  Without `home()`, the home is `E_HOME`, then `~/.e`, as for the terminal.

## Events

A turn yields events in the order they happen:

- `Text` and `Reasoning` deltas.
- `ToolCall` when the model asked for a tool, with the raw JSON arguments.
  Every call in one assistant message is announced before any runs.
- `ToolStart`.
- `ToolOutput`, a preview of live command output.
- `ToolEnd`, with the outcome and the retained content.
- `Usage` per provider request.
- `Compacting` and `Compacted` when the context window is checkpointed
  mid-turn.
- `Retry`, with the backoff.
- `Steered` and `Discarded`.
- `Named` when an extension names the session.
- `Warning`.
- `Notice` for extension messages.

Core events keep their order. e delivers a `Notice` only when no core event
is waiting. A talkative extension can interleave with model output but never
delay it.

Diagnostics raised while extensions started come out before the next turn's
first event.

## Sessions on disk

`persist(true)` writes the conversation to a JSONL log under the home's
`sessions/`. These are the same files `e -r` lists, in the documented
session format.

| API | What it does |
| --- | --- |
| `SessionBuilder::saved()` | Lists a workspace's logs. |
| `resume(path)` | Continues a log in place and holds its lock. |
| `e_sdk::transcript(path)` | Reads a log without taking ownership. |
| `history(messages)` | Seeds a session from a transcript. With `persist(true)` the seeds are written to the session file, so a resume replays the whole conversation. |

That is the whole checkpoint story: a readable file, not opaque bytes.

## Versioning

The SDK versions itself, separately from the e binary.

The crate is named `intuitums-e-sdk` because bare `e` is taken on crates.io.
The name mirrors the npm naming, where `@intuitums/e` becomes `intuitums-e`.

The SDK follows semantic versioning from its first published release.
Before 1.0, a release that changes the documented API without a compatible
path moves the minor version and names the change in the changelog.

The SDK depends on `intuitums-e-core` alone and pins the exact version it was
tested against. The core's Rust items are not a stable API. See
[Compatibility](compatibility.md).

When you change the core pin, bump the SDK's patch version too, or its minor
version if the documented API breaks. Published crate versions are
immutable.

## Why a separate package

The SDK is not part of the core and not an extension. It is a frontend over
the core, like the terminal and `e rpc`, and it never links either of them. It has its own release boundary so that
stabilizing an API is a deliberate act, not an accident of visibility.

## Building

```sh
cargo build -p intuitums-e-sdk
cargo test -p intuitums-e-sdk
cargo run -p intuitums-e-sdk --example ask -- "what does this repository do"
```

The package is a member of the root workspace, so `./x check` and `./x test`
cover it like every other member.

`./x check` also stages the files Cargo packs for both crates and compiles an
external consumer from the SDK example. That catches a missing packaged file
or a dependency drift before publication. The check patches in the staged
application crate, because its version may not be on crates.io yet. The
release job publishes the application before the SDK.

## What the SDK is not

- **Not an extension.** Extensions are child processes that speak a JSONL
  protocol to a running e. See [Extensions](extensions.md). The SDK links
  the core into your program. With `extensions(true)`, it starts the home's
  extensions for their tools and hooks. They run in the session's `cwd` and
  are told so at `initialize`. Startup hooks do not run, and no flags are
  parsed for them, because the host process's command line is not e's.
- **Not a daemon.** e stays a spawned process. There is no server to run.
- **Not a tool kernel.** The SDK runs e's tools in the working directory you
  give it, as your user, without a permission prompt. This is the same
  safety contract as the terminal. Host-defined in-process tools are not
  part of the surface. Use an extension instead.

If you integrate from another language, use the JSON output and RPC modes in
[Automation](../usage/automation.md). They remain the language-agnostic
surface. The SDK is the in-process Rust alternative.
