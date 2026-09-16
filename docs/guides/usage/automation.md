---
title: Automation
description: Run e from scripts with e -p or the e rpc session server.
order: 2
---

# Automation

e runs without a terminal in two ways. `e -p` runs one turn and exits.
`e rpc` serves many sessions over stdin and stdout.

## One turn with `e -p`

Use `e -p` to run a single turn from a script or a pipe. It prints the reply
as it streams.

```sh
e -p "what does this repo do"
git diff | e -p "review this diff"
```

The prompt is the argument. When there is no argument, e reads the prompt
from piped stdin. Warnings and the error, if any, go to stderr.

The exit status tells you how the turn ended:

| Status | Meaning |
| --- | --- |
| `0` | The turn completed. |
| `1` | An error, or an interrupted turn. |
| `2` | A usage problem. |

Pass `--no-save` to keep the session memory-only. Every other run option
applies: `--model`, `--effort`, `--no-tools`, `--image`, and
`--no-extensions`.

### JSON output

`e -p --json` streams every session event as one JSON line. The last line is
`{"type":"result", …}`, with the same fields as an `e rpc` response:

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

The event types are:

`turn_start`, `text`, `reasoning`, `tool_batch`, `tool_start`,
`tool_output`, `tool_end`, `compacting`, `compacted`, `usage`, `warning`,
`error`, `error_details`, `retry`, `recovered`, `session_name`, `steered`,
`slept`, `sleep_stopped`, `discarded`, `turn_end`.

New event types are additive. Your consumer should ignore types it does not
know.

## Many turns with `e rpc`

`e rpc` is the headless session server. A client spawns it, keeps the pipes
open, and drives sessions from any language. This is what a Slack bot, a
Linear integration, or a CI job builds on. See [Channels](channels.md).

It speaks JSONL over stdin and stdout, the same framing extensions use.
There is no port, no token, and no daemon. The process lives as long as its
client, and shutting the client down shuts e down.

Every input line is one request:

```json
{"id":"c1","method":"session.create","params":{"cwd":"/repo","model":"anthropic/claude-opus-5","save":true}}
```

Every request gets exactly one response line that carries its `id`. The
response is either `{"id":…,"result":{…}}` or `{"id":…,"error":"…"}`. The
`id` can be any JSON value.

```json
{"id":"c1","result":{"session":"01a0…","model":"anthropic/claude-opus-5","effort":"high","cwd":"/repo","path":null}}
```

To tell lines apart, check for `type`. Events are the lines with a `type`. A
line without one is a response.

### Sessions

A session is one conversation against one working directory. Sessions run
concurrently in one process, and each runs one turn at a time.

A prompt streams its events as they happen and answers when the turn ends.
Each event carries the `session` and the `request` it serves:

```json
{"id":"p1","method":"session.prompt","params":{"session":"01a0…","prompt":"what does this repo do?"}}
{"type":"turn_start","session":"01a0…","request":"p1"}
{"type":"text","delta":"It is","session":"01a0…","request":"p1"}
{"type":"tool_batch","calls":[…],"session":"01a0…","request":"p1"}
{"type":"usage","input_tokens":1200,"output_tokens":80,…,"session":"01a0…","request":"p1"}
{"type":"turn_end","aborted":false,"session":"01a0…","request":"p1"}
{"id":"p1","result":{"output":"…","final_output":"…","model":"…","effort":"high","aborted":false,"error":null,"error_details":null,"warnings":[],"usage":{…},"cost_usd":null,"tools":{"calls":1,"failures":0},"session":"01a0…","path":"/home/u/.e/sessions/…/….jsonl"}}
```

The events are the same ones `e -p --json` prints. A turn stays active
through automatic compaction and continuation, and the result describes the
completed run.

e refuses a second `session.prompt` while a turn runs. Steer or interrupt
the running turn instead.

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
session.compact    {session, focus?}             → compacting, compacted, then a result
session.set        {session, model?, effort?}    → {model, effort}  between turns; history carries over
session.messages   {session}                     → {messages:[…]}   the conversation in e's persisted shape
session.fork       {session}                     → {session, path}  a new session carrying the history
session.export     {session, path?}              → {path, title}    the conversation as one HTML page
session.close      {session}                     → {}
ask.reply          {ask, result?}                → {}               answer an extension's question
shutdown           {}                            → {}               then the process exits 0
```

e checks the type of every optional parameter before it applies a default.
`tool_mode: false` and `save: "yes"` return errors. Omit a parameter, or send
`null`, to use its default. Unknown fields are still accepted.

#### `session.create`

| Parameter | Description |
| --- | --- |
| `cwd` | The working directory, absolute or relative to the process's. It must exist. |
| `model`, `effort` | Override the process defaults set with `-m` and `--ef`. |
| `tools` | A positive allowlist of built-in tools. |
| `tool_mode` | `all` or `none`. |
| `save` | Defaults to `false`, which keeps the session memory-only. |
| `resume` | The path of a saved session. |
| `name` | A label for the session. `session.list` shows it. |

Each session can use a different `cwd`, for example one Slack channel per
repository. Trust for that directory's `AGENTS.md` works as it does in the
terminal. See [Instructions](../customize/instructions.md). A session
refuses an untrusted directory, so a caller with no terminal records the
decision first with `e trust <dir>`.

`tools` and `tool_mode` can narrow what the process allows, such as
`--no-tools`. They never widen it.

With `save` set to `true`, e writes the session log as the terminal does.
The result's `path` names the log, and `session.list` finds it later.
`--no-save` on the process wins over `save`.

Get a `resume` path from `session.list` or from an earlier result. What
happens depends on `save`:

- With `save` true, the conversation continues in that file. The file is
  locked for this process while the session is open.
- With `save` false, e loads the history without opening a writer, repairing
  the file, or taking a session lock.
- `--no-save` also keeps resume read-only.

#### `session.prompt`

`images` is a list of PNG, JPEG, GIF, or WebP paths. The limits are ten
files, 20 MiB each, and 40 MiB total. The model must declare image input.

#### `session.set` and `session.fork`

`session.set` changes the model or effort for the following turns. It does
not touch the user's saved settings. e validates the model and effort
together, so a rejected update changes neither.

`session.fork` copies the branch into a session of its own. When the
original persists, the fork gets a file of its own too. From there the two
sessions grow apart. A fork inherits the current model and effort, including
changes made with `session.set`.

### Extensions

Every session in the process shares the same extensions, as in the terminal.

An extension can ask the person something with `ui.confirm`, `ui.select`,
`ui.input`, or `ui.editor`. If the client sent `hello` with `ask: true`, the
question arrives as an `ask` line and waits for `ask.reply`:

```json
{"type":"ask","ask":1,"extension":"deploy","method":"ui.confirm","params":{"title":"Deploy?","message":"to prod"}}
{"id":"r1","method":"ask.reply","params":{"ask":1,"result":{"confirmed":true}}}
```

The `result` is whatever the extension's request expects, such as
`{"confirmed":true}`, `{"value":"a","label":"a"}`, or `{"text":"…"}`. If you
omit it, the extension gets `{"cancelled":true}`.

`ask` lines belong to the process, not to a named session. Do not infer the
owner from the most recent prompt. A client that relays questions to
separate conversations must dedicate one RPC process to each, as the Slack
channel does.

Without `hello`, or with `ask` false, e refuses questions at once with
`no ui`, as `e -p` does.

After `hello`, e handles the other extension requests this way:

- `ui.notify` and `ui.show` become `notice` lines.
- The remaining `ui.*` and `session.*` requests are refused, because they
  describe a terminal that is not there.
- Extension notices about themselves, such as a crash or a missing package,
  arrive as `{"type":"notice","message"}` lines.

Notices only appear after `hello`. A client that never greeted never sees a
line it did not ask for.

### Version 1: one-shot lines

A line without a `method` is the original one-shot request. It still works
unchanged. A fresh memory-only agent runs one turn and answers with the flat
result object, with no events.

```json
{"id":"one","prompt":"summarize this repository","model":"openai/gpt-5.5","effort":"high","tool_mode":"none","tools":null,"save":false,"images":[]}
{"id":"one","output":"...","final_output":"...","model":"provider/model","effort":"high","aborted":false,"error":null,"error_details":null,"warnings":[],"usage":{…},"cost_usd":null,"tools":{"calls":2,"failures":0},"session":null}
```

`prompt` is required and must not be empty. With `save` true, e persists the
turn and `session` is its JSONL path. Every other field works as it does for
`session.create`.

One-shot responses come in the order the turns end. For a caller that waits
for each response, that is input order.

### Process options and limits

Add `--no-tools` (`--nt`) for a no-tool policy. Add `--no-extensions`
(`--ne`) when startup must be hermetic.

Tool batches run in bounded waves. Set the wave size with `tool_concurrency`
in `~/.e/settings.json`. The default is 8, and the range is 1 to 64. Calls
that name the same file run in provider order.

A request line is at most 10 MiB. A malformed line produces one
`{"id":null,"error":"…"}` line, and the process keeps serving. An oversized
line ends the process, since the stream has no safe point to resync past it.

### Usage and errors

Usage categories in results are disjoint. `input_tokens` excludes cache
reads and writes. `prompt_tokens` is the complete sum of input, cache reads,
and cache writes. Compaction requests are included.

A terminal provider failure adds `error_details` beside the compatible
`error` string. The details hold the stage, provider metadata, the retry
decision, and bounded diagnostic text. With a saved session, e also appends
the same details to a private `.errors.jsonl` sidecar. Nothing is uploaded.

### Shutdown

EOF on stdin, or `shutdown`, stops every session, shuts extensions down, and
exits 0.

On Unix, SIGTERM and SIGHUP first kill every built-in bash process group,
including detached children of the shell. Then e shuts extensions down and
exits with status 143 or 129.
