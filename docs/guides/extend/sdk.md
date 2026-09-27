---
title: SDK
description: Embed e's agent core in your Rust programs.
order: 3
---

# SDK

The SDK is e's coding agent as a Rust library, `e-sdk` in `crates/sdk/`.
Read this guide to run e sessions inside your own Rust program; from another
language, use `e rpc` instead (see [automation](../usage/automation.md)).

A session runs the same core as the terminal, without a terminal: the
built-in tools, skills and `AGENTS.md` context, automatic compaction,
on-disk session logs, and optionally the user's extensions.

## Add the dependency

`e-sdk` is not published to crates.io yet. Depend on it from the
repository:

```sh
cargo add e-sdk --git https://github.com/arocomputer/e
cargo add tokio --features rt-multi-thread,macros
```

Pin a commit with `--rev <sha>` for reproducible builds. The crate needs the
Rust version the repository's `Cargo.toml` declares (`rust-version`) and a
Tokio runtime: the core spawns its turn worker and runs tool I/O on the
blocking pool.

A session uses the models and credentials in e's home, so sign in to a
provider with `e` and `/login` first, or pass `.home()` a home that has
them.

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

`crates/sdk/examples/ask.rs` is a complete program; in a checkout, run it
with `cargo run -p e-sdk --example ask -- "what does this repository do"`.

## `SessionBuilder`

`Session::builder()` returns a `SessionBuilder`. Every option has a default.

| Method | Default | Meaning |
| --- | --- | --- |
| `cwd(path)` | the process's working directory | The workspace the tools operate in and whose `AGENTS.md` and skills load. |
| `home(path)` | `E_HOME`, else the build's home | The configuration home: credentials, models, settings, extensions, and where sessions persist. Scoped to this session; the process environment is untouched, so sessions with different homes coexist. |
| `model(query)` | the configured default | `provider/id`, a bare id, or a unique substring of an available model. |
| `effort(level)` | the model's default | One of the model's declared reasoning levels. Local to this process; never saved. |
| `tools(Tools)` | `Tools::All` | `Tools::All` (built-ins plus extension tools), `Tools::None`, or `Tools::Only(vec![…])` (built-ins only, enforced at execution). |
| `persist(bool)` | `false` | Write the conversation to a JSONL log under the home's `sessions/`. |
| `extensions(bool)` | `false` | Start the home's extensions. See [Extensions in the SDK](#extensions-in-the-sdk). |
| `instructions(text)` | none | Appended to e's system prompt, after the skills catalog and project context. |
| `resume(path)` | none | Continue a saved session file in place. Implies `persist(true)` and holds the file's lock. |
| `history(messages)` | empty | Seed the conversation. Ignored with `resume`. With `persist(true)`, the seeds are written to the new session file. |
| `workspace(Arc<dyn Workspace>)` | the disk | Where the file tools read and write. See [Workspaces and shells](#workspaces-and-shells). |
| `shell(Arc<dyn Shell>)` | local processes | Where `bash` commands run. |
| `api_key(key)` | the home's credentials | Authenticates the model with this key instead. `model` may then name any declared model, signed in or not. The key is never written anywhere. |

`saved()` lists this workspace's saved sessions in the home, newest first.
`build().await` starts the session.

## `Session`

A `Session` is one conversation, prompted many times.

| Method | Meaning |
| --- | --- |
| `prompt(p)` | Start a `Turn`. Takes a `&str`, a `String`, or a `Prompt` (`Prompt::new(text).image(img)` or `.image_file(path)?`). |
| `compact()` | A `Turn` that summarizes older history now; it emits `Compacting` and `Compacted`. |
| `model()`, `set_model(query)` | The active model as `provider/id`; switch between turns. |
| `effort()` | The reasoning effort the next request uses. |
| `history()` | The conversation so far, as `Message` values. |
| `clear()` | Forget the conversation. |
| `path()` | The session log's path, once a persisted session has its first message. |
| `cwd()` | The workspace. |
| `close().await` | Stop leftover work and shut extensions down. Dropping a session does this on a best-effort basis. |

`prompt` borrows the session mutably for the turn's lifetime, so only one
turn runs at a time.

## `Turn`

A `Turn` is one prompt's run. It is lazy: nothing is sent until you first
poll it.

- Call `next().await` for each `Event` (it is also a `futures::Stream`),
  then `finish().await` for the `Reply`.
- Or `.await` the turn directly to skip the events.
- `steer(text)` adds a message that e delivers before the turn's next
  provider request.
- `cancel()` works like pressing Esc. Dropping a running turn also
  interrupts it; the session stays usable.

### Events

A turn yields events in the order they happen:

| Event | Meaning |
| --- | --- |
| `Text(String)`, `Reasoning(String)` | Reply and visible-reasoning deltas. |
| `ToolCall { id, name, arguments }` | The model asked for a tool; `arguments` is the raw JSON. Every call in one assistant message is announced before any runs. |
| `ToolStart { id }` | The call started executing. |
| `ToolOutput { id, stream, chunk }` | A preview of live command output. A slow reader may miss chunks. |
| `ToolEnd { id, outcome, summary, content }` | The call finished; `content` is what the model reads. |
| `Usage(Usage)` | Token counts for one provider request. |
| `Compacting`, `Compacted { summary, context_tokens }` | History is being, and has been, checkpointed. |
| `Retry { attempt, limit, delay, reason }` | A retryable provider failure and the backoff before the next attempt. |
| `Steered(String)`, `Discarded(Vec<String>)` | A steering message was taken up, or never ran because the turn ended first. |
| `Named(String)` | An extension named the session. |
| `Warning(String)` | A non-fatal problem, also collected in the reply. |
| `Notice(String)` | An extension message or startup diagnostic. |

The turn reads the core's event channel directly, so an unread event holds
the model instead of growing a buffer. A `Notice` is delivered only when no
core event is waiting, so a talkative extension never delays model output.
Diagnostics from extension startup arrive before the next turn's first
event.

### `Reply`

`Reply` holds the joined assistant `text`, summed `usage`, an optional
`cost_usd` estimate from the model's pricing, `tools` (`calls` and
`failures`), `stop` (`Stop::Complete` or `Stop::Cancelled`), and
`warnings`.

## Errors

`build()` checks everything it can up front and returns an `Error`:
`ModelUnavailable`, `NoProvider`, `KeyWithoutModel` (`api_key` without
`model`), `Effort` (unsupported level),
`UnknownTool`, `Cwd` (unusable working directory), or `Session` (a session
file that cannot be opened, locked, or read). `Prompt::image_file` returns
`Error::Image`.

A turn that ran and failed returns a `TurnError`. Its `reply` holds
everything the turn produced before it failed: text, usage, and tool counts.
A core error is not an event; it ends the turn and arrives as the
`TurnError`.

## Sessions on disk

Nothing is written to e's home unless you ask: conversations are
memory-only without `persist(true)`, and the SDK never writes settings.
With `persist(true)`, the log is the same JSONL file `e -r` lists, in the
[session format](../usage/sessions.md).

| API | What it does |
| --- | --- |
| `SessionBuilder::saved()` | Lists a workspace's logs. |
| `SessionBuilder::resume(path)` | Continues a log in place and holds its lock. |
| `e_sdk::transcript(path)` | Reads a log's active conversation without taking ownership. |
| `SessionBuilder::history(messages)` | Seeds a session, for example from `transcript`. |

## Workspaces and shells

By default the tools work on the disk and `bash` starts local processes, as
your user. Give a session its own `Workspace` and `Shell` to put them
somewhere else: an in-memory tree for a test or a preview, a container's
filesystem and shell, a remote machine. `cwd` then names a directory inside
that workspace.

```rust
use std::sync::Arc;
use e_sdk::{MemoryWorkspace, Session, Workspace};

# async fn demo() -> Result<(), e_sdk::Error> {
let workspace = Arc::new(MemoryWorkspace::new());
workspace.create_dir_all("/project".as_ref())?;
workspace.write("/project/main.rs".as_ref(), b"fn main() {}\n")?;
let mut session = Session::builder()
    .cwd("/project")
    .workspace(workspace.clone())
    .build()
    .await?;
session.prompt("Add a greeting to main.rs").await?;
println!("{}", workspace.read_to_string("/project/main.rs".as_ref())?);
# Ok(())
# }
```

A `Workspace` implements eight file operations: metadata (following links
and not), open, write, remove, create directories, list a directory, and
canonicalize. `MemoryWorkspace` and `DiskWorkspace` are the two built in. A
`Shell` runs one command in a directory and resolves with its stdout,
stderr, and exit code; it should see the same files as the workspace. With
a shell, `bash` refuses background commands, which need a local process to
track.

Project resources (`AGENTS.md`, skills, prompt templates) are not read from
a supplied workspace. They load from the disk at `cwd`, and only when you
trusted that directory. Pass a workspace's own guidance with
`instructions(text)`.

## Extensions in the SDK

With `extensions(true)`, the session starts the home's
[extensions](extensions.md) for their tools and hooks. They run in the
session's `cwd` and are told so at `initialize`, with `ui: false`, so every
`ui.*` and `session.*` request fails with `no ui`. Startup hooks do not run
and no flags are parsed, because the host process's command line is not
e's.

The SDK runs e's tools in the working directory you give it, as your user,
without a permission prompt, the same safety contract as the terminal,
unless you give the session a [workspace and shell](#workspaces-and-shells)
of your own. There are no host-defined in-process tools; write an extension
instead.

## Versioning

The SDK versions itself, separately from the e binary. It depends on
`e-core` alone and pins the exact version it was tested against; the
core's Rust items are not a stable API (see
[Compatibility](compatibility.md)).

The SDK follows semantic versioning from its first published release.
Before 1.0, a release that changes the documented API without a compatible
path moves the minor version and names the change in the changelog. When
you change the core pin, bump the SDK's patch version too, or its minor
version if the documented API breaks.

## Working on the SDK

```sh
cargo build -p e-sdk
cargo test -p e-sdk
```

The SDK is a workspace member, so `./x check` and `./x test` cover it.
`./x check` also compiles an external consumer from the packed SDK crate,
which catches a missing packaged file or dependency drift.
