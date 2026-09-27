---
title: Automation
description: Run e from scripts with e -p or the e rpc session server.
order: 2
---

# Automation

This guide covers running e without a terminal: `e -p` runs one turn and
exits, and `e rpc` serves many sessions to a program over stdin and stdout.
Read it when you script e, run it in CI, or build a client such as a
[channel](channels.md).

Both refuse a working directory that is not trusted, because nothing can
answer the trust panel. Record the decision first with `e trust <dir>`. See
[instructions](../customize/instructions.md).

## One turn with `e -p`

```sh
e -p "what does this repo do"
git diff | e -p "review this diff"
```

The prompt is the argument. With no argument, e reads the prompt from piped
stdin. The reply streams to stdout; warnings and the error, if any, go to
stderr.

| Exit status | Meaning |
| --- | --- |
| `0` | The turn completed. |
| `1` | An error, an interrupted turn, or an untrusted directory. |
| `2` | A usage problem, such as an empty prompt or an unknown model. |

The session is saved like a terminal session unless you pass `--no-save`.
The other run options apply too: `--model`, `--effort`, `--no-tools`,
`--image`, `--package`, and `--no-extensions`. See
[command line](commands.md#run-options). Extensions start without a UI, so
their questions are refused.

### JSON output

`e -p --json` prints every session event as one JSON line. The last line is
`{"type":"result", …}` with the same fields as an `e rpc` turn result, where
`session` is the saved session's path or `null`:

```json
{"type":"turn_start"}
{"type":"text","delta":"The repo"}
{"type":"tool_batch","calls":[{"id":1,"name":"read","arguments":{"path":"README.md"},"category":"read","target":"README.md"}]}
{"type":"tool_start","id":1}
{"type":"tool_end","id":1,"outcome":"completed","summary":"12 lines","content":"…"}
{"type":"usage","input_tokens":1200,"output_tokens":80,"cache_read_tokens":0,"cache_write_5m_tokens":0,"cache_write_1h_tokens":0}
{"type":"turn_end","aborted":false}
{"type":"result","output":"…","final_output":"…","model":"provider/model","effort":"high","aborted":false,"error":null,"error_details":null,"warnings":[],"usage":{…},"cost_usd":null,"tools":{"calls":1,"failures":0},"session":null}
```

A usage problem prints `{"type":"result","error":"…"}` and exits 2. The
[events](#events) and the [turn result](#turn-result) are described below.

## Many turns with `e rpc`

`e rpc` is the headless session server. A client spawns it, keeps the pipes
open, and drives sessions from any language. It speaks JSONL, one JSON object
per line, the same framing extensions use. There is no port, no token, and no
daemon: the process lives as long as its client.

### Requests and responses

Every input line is one request with an `id`, a `method`, and optional
`params`:

```json
{"id":"c1","method":"session.create","params":{"cwd":"/repo","model":"anthropic/claude-opus-5","save":true}}
```

Every request gets exactly one response line with the same `id`, either
`{"id":…,"result":{…}}` or `{"id":…,"error":"…"}`. The `id` can be any JSON
value.

```json
{"id":"c1","result":{"session":"01a0…","model":"anthropic/claude-opus-5","effort":"high","cwd":"/repo","path":null}}
```

Lines with a `type` are events or notices; a line without one is a response.

### Run a turn

A session is one conversation against one working directory. Sessions run
concurrently in one process, and each runs one turn at a time.

`session.prompt` streams the turn's events as they happen and answers when the
turn ends. Each event carries the `session` and the `request` it serves:

```json
{"id":"p1","method":"session.prompt","params":{"session":"01a0…","prompt":"what does this repo do?"}}
{"type":"turn_start","session":"01a0…","request":"p1"}
{"type":"text","delta":"It is","session":"01a0…","request":"p1"}
{"type":"tool_batch","calls":[…],"session":"01a0…","request":"p1"}
{"type":"usage","input_tokens":1200,"output_tokens":80,…,"session":"01a0…","request":"p1"}
{"type":"turn_end","aborted":false,"session":"01a0…","request":"p1"}
{"id":"p1","result":{"output":"…","final_output":"…","model":"…","effort":"high","aborted":false,"error":null,"error_details":null,"warnings":[],"usage":{…},"cost_usd":null,"tools":{"calls":1,"failures":0},"session":"01a0…","path":"/home/u/.e/sessions/…/….jsonl"}}
```

A turn stays active through automatic compaction and continuation, and the
result describes the completed run.

While a turn runs, e refuses a second `session.prompt`, and also
`session.compact`, `session.set`, and `session.fork`. Use `session.steer` to
add a message to the running turn, or `session.interrupt` to stop it.

### Methods

```
hello              {ask?}                        → {protocol, version, channel, commit, cwd, home, methods, ask}
models.list        {}                            → {default, models:[{model, provider, id, effort, image_input, tools, context_window}]}
session.create     {cwd?, model?, effort?, tools?, tool_mode?, save?, resume?, name?}
                                                 → {session, model, effort, cwd, path}
session.list       {cwd?, all?}                  → {sessions:[{path, title, name, modified, messages, turns, cwd}]}
session.info       {session}                     → {session, model, effort, cwd, path, name, messages, running}
session.prompt     {session, prompt, images?}    → events…, then the turn result
session.steer      {session, text}               → {held:true}       a message into the running turn
session.interrupt  {session}                     → {}
session.compact    {session, focus?}             → compacting, compacted, then a turn result
session.set        {session, model?, effort?}    → {model, effort}   between turns; history carries over
session.messages   {session}                     → {messages:[…]}    the conversation in e's persisted shape
session.fork       {session}                     → {session, path}   a new session carrying the history
session.export     {session, path?}              → {path, title}     the conversation as one HTML page
session.close      {session}                     → {}
ask.reply          {ask, result?}                → {}                answer an extension's question
shutdown           {}                            → {}                then the process exits 0
```

`hello` reports `protocol: 2`. New fields and methods are additive and do not
change it.

e checks the type of every parameter before it applies a default, so
`tool_mode: false` and `save: "yes"` are errors. Omit a parameter, or send
`null`, to use its default. Unknown fields are accepted.

#### `session.create`

| Parameter | Description |
| --- | --- |
| `cwd` | The working directory, absolute or relative to the process's. It must exist and be trusted. Default: the process's directory. |
| `model`, `effort` | Override the process defaults set with `-m` and `--ef`. |
| `tools` | A positive allowlist of built-in tools: `read`, `write`, `edit`, `grep`, `bash`, `read_result`. An unknown name is an error. |
| `tool_mode` | `all` (default) or `none`. |
| `save` | `true` writes a session log. Default `false`: memory only. |
| `resume` | The path of a saved session to continue. |
| `name` | A label for the session. `session.list` and `/resume` show it. |

Each session can use a different `cwd`, for example one per repository.

`tools` and `tool_mode` can narrow what the process allows but never widen
it: under `--no-tools`, `tool_mode: "all"` still runs no tools. Likewise
`--no-save` on the process wins over `save`.

With `save: true`, e writes the session log as the terminal does. The
result's `path` names the log, and `session.list` finds it later.

Get a `resume` path from `session.list` or an earlier result's `path`:

- With `save: true`, the conversation continues in that file, which stays
  locked to this process while the session is open.
- With `save` false, or under `--no-save`, e loads the history read-only: no
  writer, no repair, no lock.

#### `session.list`

Lists saved sessions, newest first, for `cwd` (default: the process's
directory), or for every workspace with `all: true`.

#### `session.prompt`

`images` is a list of PNG, JPEG, GIF, or WebP paths: at most ten files,
20 MiB each, 40 MiB in total. The model must declare image input.

#### `session.set`

Changes the model or effort for the following turns without touching the
user's saved settings. e validates the model and effort together, so a
rejected update changes neither.

#### `session.fork`

Copies the conversation into a new session. When the original is saved, the
fork gets a file of its own. From there the two grow apart. The fork inherits
the current model, effort (including changes made with `session.set`), and
name.

#### `session.export`

Writes the conversation as one self-contained HTML page. `path` is resolved
against the session's `cwd`; the default is `e-session-<id>.html` there. The
result gives the `path` and the page `title`.

#### `session.close`

Forgets the session and interrupts its turn. A `session.prompt` still waiting
is answered with `{"id":…,"error":"session closed"}`.

### Extension questions

Every session in the process shares the same extensions, as in the terminal.

An extension can ask the person something with `ui.confirm`, `ui.select`,
`ui.input`, or `ui.editor`. If the client sent `hello` with `ask: true`, the
question arrives as an `ask` line and waits for `ask.reply`:

```json
{"type":"ask","ask":1,"extension":"deploy","method":"ui.confirm","params":{"title":"Deploy?","message":"to prod"}}
{"id":"r1","method":"ask.reply","params":{"ask":1,"result":{"confirmed":true}}}
```

The `result` is whatever the extension's request expects, such as
`{"confirmed":true}`, `{"value":"a","label":"a"}`, or `{"text":"…"}`. Omit it
and the extension gets `{"cancelled":true}`.

`ask` lines belong to the process, not to a session, so do not infer the
owner from the most recent prompt. A client that relays questions to
separate conversations runs one `e rpc` process per conversation, as the
Slack channel does.

Without `hello`, or with `ask: false`, e refuses questions at once with
`no ui`, as `e -p` does.

### Notices

After `hello`, e also writes lines nobody requested:

| Line | When |
| --- | --- |
| `{"type":"notice","extension","method","params"}` | an extension called `ui.notify` or `ui.show` |
| `{"type":"notice","message"}` | e reports on an extension itself, such as a crash or a missing package |

Every other `ui.*` and `session.*` extension request is refused, because it
describes a terminal that is not there. A client that never sends `hello`
sees only responses and the events of its own prompts.

### One-shot requests (version 1)

A line without a `method` is the original one-shot request, and it still
works. A fresh memory-only session in the process's directory runs one turn
and answers with the flat turn result, with no events:

```json
{"id":"one","prompt":"summarize this repository","model":"openai/gpt-5.5","effort":"high","tool_mode":"none","tools":null,"save":false,"images":[]}
{"id":"one","output":"...","final_output":"...","model":"provider/model","effort":"high","aborted":false,"error":null,"error_details":null,"warnings":[],"usage":{…},"cost_usd":null,"tools":{"calls":2,"failures":0},"session":null}
```

`prompt` is required and must not be empty. With `save: true`, e persists
the turn and `session` is its JSONL path. The other fields work as they do
for `session.create` and `session.prompt`.

One-shot responses come in the order the turns end. For a caller that waits
for each response, that is input order.

### Process options and limits

`e rpc` takes the run options as process defaults for every session:
`--model`, `--effort`, `--no-tools`, `--no-save`, `--package`, and
`--no-extensions` (for a hermetic startup).

Tool calls in one batch run in waves of up to `tool_concurrency` (in
`~/.e/settings.json`; default 8, range 1 to 64). Calls that name the same
file run in provider order.

A request line is at most 10 MiB. A malformed line gets one
`{"id":null,"error":"…"}` response and the process keeps serving. An
oversized line ends the process, since the stream has no safe point to
resync past it.

### Shutdown

| Trigger | Effect |
| --- | --- |
| EOF on stdin | No new requests. Running turns finish and answer, then e exits 0. |
| `shutdown` | e answers `{}`, stops every session at once, and exits 0. |
| SIGTERM, SIGHUP (Unix) | e stops at once and exits 143 or 129. |

In every case e first kills the process groups of the built-in `bash` tool,
including detached children of the shell, then shuts extensions down.

## Events

`e -p --json` and `session.prompt` emit the same events. New event types are
additive, so a consumer should ignore types it does not know.

| `type` | Fields |
| --- | --- |
| `turn_start` | |
| `text` | `delta`: reply text |
| `reasoning` | `delta`: reasoning text |
| `tool_batch` | `calls`: `[{id, name, arguments, category, target}]` |
| `tool_start` | `id` |
| `tool_output` | `id`, `stream` (`stdout` or `stderr`), `chunk` |
| `tool_end` | `id`, `outcome` (`completed`, `failed`, `timedout`, `blocked`, `cancelled`), `summary`, `content` |
| `compacting` | |
| `compacted` | `summary`, `context_tokens` |
| `usage` | `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_5m_tokens`, `cache_write_1h_tokens` |
| `warning` | `message` |
| `error` | `message` |
| `error_details` | `details` |
| `retry` | `attempt`, `limit`, `delay_secs`, `cause`, `reason` |
| `recovered` | `attempt`, `limit` |
| `session_name` | `name`: an extension named the session |
| `instructions` | `path`: a nested `AGENTS.md` added because a tool touched a path under it |
| `steered` | `text` |
| `slept`, `sleep_stopped` | `duration_secs` |
| `discarded` | `prompts`: queued prompts that could not run |
| `turn_end` | `aborted` |

## Turn result

| Field | Meaning |
| --- | --- |
| `output` | everything the reply streamed |
| `final_output` | the reply when the turn completed cleanly, else `""` |
| `model`, `effort` | what the turn ran with |
| `aborted` | the turn was interrupted |
| `error`, `error_details` | a terminal failure, or `null` |
| `warnings` | configuration warnings, `warning` events, and retries |
| `usage` | `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_5m_tokens`, `cache_write_1h_tokens`, `prompt_tokens` |
| `cost_usd` | estimated cost, or `null` when the model has no pricing |
| `tools` | `{calls, failures}` |
| `session`, `path` | see each surface above |

Usage categories are disjoint: `input_tokens` excludes cache reads and
writes, and `prompt_tokens` is the sum of input, cache reads, and cache
writes. Compaction requests are included.

`error_details` sits beside the `error` string for a terminal provider
failure. It holds the stage, provider metadata, the retry decision, and
bounded diagnostic text. For a saved session, e also appends the details to a
private `.errors.jsonl` file beside the log. Nothing is uploaded.
