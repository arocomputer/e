---
title: Extensions
description: Add tools, commands, hooks, and UI over a JSON line protocol.
order: 1
---

# Extensions

An extension is a program that adds tools, commands, hooks, and UI to e. It
is an executable in `~/.e/extensions/`, or in the `extensions/`
directory of an installed [package](packages.md). It can be a top-level file
such as `foo.mjs`. It can also be the entry point of a directory such as
`foo/` that holds helper files too.

e picks a directory's entry point by checking, in order:

1. `index.*`
2. a file matching the directory name
3. a sole executable

When two files match the same rule, path order breaks the tie.

You can write an extension in any language. e starts each process at launch
and keeps it running for the session. The two sides exchange one JSON object
per line over stdin and stdout.

## What extensions can do

- Add tools the model calls. A tool with a built-in's name overrides that
  built-in.
- Add slash commands that show up in the `/` picker, and add shortcuts.
- Rewrite or swallow a submitted line with the `input` hook.
- Name the session from a command or tool result. `/resume` shows the name.
- Gate tool calls with the `tool_call` hook, which returns a block and a
  reason.
- Shape a turn with the `before_turn`, `tool_result`, and `compact_summary`
  hooks.
- Subscribe to lifecycle events for the session, turns, tools, compaction,
  and the model.
- Show a notice, a block of text, markdown, a real diff, a tool row with its
  own verbs, a panel, or a status slot.
- Ask the user to pick from a list, answer yes or no, or type a line.
- Steer the session by injecting a message, narrowing the toolset, switching
  the model or effort, interrupting, or compacting.
- Handle startup arguments and request a relaunch of the same binary in
  another directory.

Everything an extension shows is data, and e paints it through the user's
theme. An extension never emits terminal bytes and never runs inside e. e
reports a crashed or hostile extension as a notice, and the extension never
gets control of the terminal.

## Wire protocol

The protocol is version 1, extended by capabilities. Messages flow in both
directions, one JSON object per line.

### Requests from e

Each request carries an `id`. Your extension answers with that `id`.

```
{"id":1,"method":"initialize","params":{"protocol":1,"capabilities":["tool.update","events","hooks","display","ui","session","shortcuts","pane","widget","render"],"ui":true,"e_version":"0.0.1","cwd":"/path","extensions_config":{…}}}
{"id":2,"method":"hook.startup","params":{"cwd":"/path","argv":["--project","../app"],"flags":{"project":"../app"}}}
{"id":3,"method":"tool_call","params":{"name":"greet","arguments":{...}}}
{"id":4,"method":"command","params":{"name":"ping","args":"rest of the line"}}
{"id":5,"method":"hook.tool_call","params":{"name":"bash","arguments":{...}}}
{"id":6,"method":"hook.input","params":{"text":"a submitted line"}}
{"id":7,"method":"hook.before_turn","params":{"prompt":"the user's message"}}
{"id":8,"method":"hook.tool_result","params":{"name":"bash","content":"…","is_error":false}}
{"id":9,"method":"hook.compact_summary","params":{"summary":"…"}}
{"id":11,"method":"hook.render","params":{"kind":"tool","name":"bash","content":"…"}}
{"id":10,"method":"shortcut","params":{"key":"ctrl+alt+g"}}
```

### Notifications from e

Notifications have no `id` and take no reply.

```
{"method":"event","params":{"name":"turn_end","extra":{"aborted":false}}}
{"method":"flags","params":{"flags":{…}}}
{"method":"ui.key","params":{"key":"down"}}          while your interactive panel is open
{"method":"ui.panel_closed","params":{}}             the user closed it
{"method":"pane.select","params":{"pane":"diff","section":"files","id":"a.rs"}}   the side pane's cursor moved
{"method":"pane.activate","params":{"pane":"diff","section":"files","id":"a.rs"}} Enter on a pane item
{"method":"pane.key","params":{"pane":"diff","key":"x"}}                          a pane chord e did not use
{"method":"pane.closed","params":{"pane":"diff"}}                                 the user closed the pane
{"method":"shutdown"}
```

### Messages from your extension

```
{"id":1,"result":{...}}                        answer a request
{"id":2,"error":"what went wrong"}             or fail it
{"method":"notify","params":{"message":"hi"}}  a transcript notice, any time
{"method":"tool.update","params":{"id":3,"stream":"stdout","chunk":"working\n"}}
{"id":"q1","method":"ui.select","params":{…}}  ask e something (see below)
```

A request from your extension carries its own `id`, which can be any JSON
value. e answers with the same id: `{"id":"q1","result":{…}}` or
`{"id":"q1","error":"…"}`. The two id spaces never meet, because direction
tells them apart.

### Capabilities and `ui`

In the `initialize` params, `capabilities` lists the families this e speaks.
`ui` says whether someone can answer `ui.*` requests:

- Under `e -p`, `ui` is false. e answers every `ui.*` request with
  `{"error":"no ui"}` at once.
- Under `e rpc`, `ui` is true, and the client may relay questions to a
  person. See [automation](../usage/automation.md). e still refuses the
  display-only requests there.

Handle the error in both cases.

## Results by method

Each request from e expects a result of a specific shape.

### `initialize`

Your answer to `initialize` is the manifest. Everything but `name` is
optional. `parameters` is a JSON Schema object.

```json
{
  "name": "my-ext",
  "version": "1.0",
  "tools": [
    {
      "name": "greet",
      "description": "say hi",
      "parameters": {"type": "object", "properties": {}},
      "label": {"category": "greet", "running": "Greeting", "completed": "Greeted", "target": "who"}
    }
  ],
  "commands": [{"name": "ping", "description": "check the extension"}],
  "flags": [
    {"name": "project", "type": "string", "description": "relaunch in this directory"},
    {"name": "plan", "type": "boolean", "description": "plan mode"}
  ],
  "hooks": ["tool_call", "input", "before_turn", "tool_result", "compact_summary", "render"],
  "renders": ["tool:bash", "assistant"],
  "events": ["session_start", "turn_start", "tool_end"],
  "shortcuts": [{"key": "ctrl+alt+g", "description": "greet"}]
}
```

The `initialize` params carry your configuration in `extensions_config`. It
holds every entry under `"extensions"` in `~/.e/settings.json`, namespaced by
extension name. Your extension gets its own config without claiming a
top-level settings key.

A tool's `label` gives its transcript row the built-in grammar. The example
row reads `Greeting bob` while running and `Greeted bob` after.

| Field | Meaning |
| --- | --- |
| `category` | The noun used when e tallies a batch of tool calls. |
| `running`, `completed` | Verbs, shown as given. |
| `target` | The argument whose value the row shows. |

Without a label, the row reads `Running greet` and then `Ran greet`.

### Flags

Declare `flags` so they are discoverable and so e can parse them for you. A
flag's `type` is `"boolean"`, the default, or `"string"`. e recognizes both
types in startup argv:

- A boolean matches `--name`, `--name=true`, `--name=false`, and `--no-name`.
- A string matches `--name=value` or `--name value`. e never consumes a
  following `-` token as the value.
- A bare string flag at the end of argv parses as `null`. The flag is
  present but has no value.
- The last occurrence wins.
- `--` stops parsing.

A name that isn't a clean `--name` token, such as `"-x, --example"`, appears
in `e --help`, but e never parses it. Those flags still need the startup
hook's raw argv.

After every startup hook has seen the raw argv, e removes typed flags and
their separated string values. Only then does e parse its own subcommands and
build the initial prompt.

Right after launch, e sends the parsed flags as a `flags` notification to
every extension that declares typed flags. No reply is needed. A tool-only
extension can read them from any handler, not just during startup. The raw
protocol message is `{"method":"flags","params":{"flags":{…}}}`.

The notification carries only flags actually passed on the command line. An
absent flag stays absent, so a handler can tell "passed false" from "not
passed".

A declaration can set an optional `"default"`, the value to use when the flag
is absent. e retains the default but never adds it to the notification. Your
extension applies it itself. The scaffold helper does this for you:

- `flag(name)` returns the passed value, else the declared default, else
  undefined.
- `flagPassed(name)` is true only when the flag was on the command line,
  regardless of default.

### `tool_call`

Your answer to `tool_call` is the tool's result:

```json
{"content":"text the model sees","is_error":false,"session_name":"optional new session name","summary":"+12 -3","display":"…","format":"diff"}
```

| Field | Meaning |
| --- | --- |
| `content` | All the model reads. |
| `session_name` | An optional new session name. |
| `summary` | The suffix on the tool's row. |
| `display` | What the ctrl+o viewer shows instead of `content`. A built-in edit does the same with its full diff. |
| `format` | `text`, the default, `markdown`, or `diff`. |

With `diff`, e takes a unified diff, the output of `git diff`, and converts
it to the viewer's row grammar with real line numbers and coloured markers.

Before the final response, your extension may emit any number of
`tool.update` notifications to stream output:

- `id` must be the active tool-call request id.
- `stream` is `stdout` or `stderr`.
- e displays `chunk` through the same ordered tool-output stream as built-in
  commands.

Version-1 extensions remain compatible. They simply never emit an update. The
scaffold passes tool handlers a second `{update}` argument:

```js
async tool({ arguments }, { update }) {
  update("starting\n");
  update("a warning\n", "stderr");
  return { content: "done" };
}
```

### `command`

Your answer to `command` holds any combination of these:

- `{"notice":"line for the transcript"}`
- `{"show":{"title":"diff src/main.rs","body":"…","format":"diff"}}`, a block
  in the transcript. See `ui.show`.
- `{"prompt":"text submitted as the user"}`

It can also set `{"session_name":"name shown in /resume"}`.

A command may declare `"arguments":"<env>"`. The `/` picker shows it, and
picking the command leaves `/name ` in the composer for the user to finish.

A command may also declare `"completions":true`. Typing `/name pre` then
sends
`{"id":…,"method":"command.complete","params":{"name":"name","prefix":"pre"}}`.
Your answer, `{"items":[{"value":"prefix-match","label"?,"description"?}]}`,
opens a picker, and the chosen item replaces the prefix. Completions have
three seconds. A slow or empty answer shows nothing.

### `shortcut`

Answer a shortcut with the same result shape as a command. e sends it to the
extension that declared the chord. See [Shortcuts](#shortcuts).

### Pane notifications

`pane.select`, `pane.activate`, `pane.key`, and `pane.closed` are
notifications from the side pane. See [The side pane](#the-side-pane).

### `hook.before_turn`

Use this hook to add context to a turn. e runs it once per turn, before the
first request, with the prompt that started the turn. Answer:

```json
{"system_suffix":"a paragraph appended to the system prompt for this turn","message":{"content":"…","internal":true}}
```

e appends `system_suffix` to the system prompt and never replaces the prompt.
The system prompt is the user's file-backed contract with the model.

e adds `message` to the conversation before the request. `internal` is the
default and keeps the message out of the transcript.

### `hook.tool_result`

Use this hook to redact or trim tool output. e runs it after every tool,
before the result is shown, stored, or sent. Answer
`{"content":"what the model should read instead"}`, or `{}` to keep the
result.

Extensions see each other's rewrites in declaration order. A rewrite also
drops the tool's richer `display` text, so the viewer shows exactly what you
let through.

### `hook.render`

Use this hook to supply the content for an entry that e renders. Declare
`renders` in the manifest to be asked:

- `"tool:bash"`, or `"tool:*"`, asks for a tool's finished result. Your body
  replaces it in the ctrl+o viewer.
- `"assistant"` asks for a completed reply. Your body replaces its markdown
  in the transcript.

The params are `{kind: "tool"|"assistant", name, content}`. Answer
`{"body":"…","format":"text"|"markdown"|"diff"}`, or `{}` to leave the entry
as e paints it.

Extensions see each other's answers in declaration order. A slow answer
changes nothing.

### `hook.compact_summary`

The generated summary is about to replace the older conversation, and this
hook has the last word on it. Answer `{"summary":"…"}` or `{}`.

### `hook.tool_call`

Answer `{"block":true,"reason":"why"}` to stop the call. The model sees the
reason as an error result. Answer `{"block":false}` to allow the call.

### `hook.input`

This hook decides what happens to a submitted line. e runs input hooks in
order, and the first extension to consume or replace the line wins:

```json
{"consume":true,"notice":"swallowed, with a notice"}
{"replace":"the rewritten line"}
{"consume":false,"replace":null}
```

An empty result lets the line through untouched. `{"notice":"…"}` lets the
line through and posts the notice. The transcript shows notices from every
extension that allowed the line, alongside the notice from whichever
extension consumed or replaced it.

e handles a pasted API key before the hook, so the key never reaches it.

### `hook.startup`

This hook rewrites arguments and optionally changes the process. It receives
`{cwd, argv, flags}`, where `flags` holds the parsed values of every typed
flag declaration. Answer:

```json
{"argv":["-c"],
 "env":{"REMOVE_ME":null},
 "relaunch":{"cwd":"/path/to/project","env":{"BOOTSTRAPPED":"1"}}}
```

Startup hooks run in extension filename order, before e parses subcommands,
`-c`, `-r`, or the initial prompt.

- `argv` feeds the next hook.
- `env` changes the current process.
- `relaunch` replaces the current process with the same e binary in `cwd`.
  Extensions cannot choose another executable. The first relaunch ends the
  chain.

## Events

Events tell your extension what happens in a session. List the events you
want in the manifest's `events`. e sends only those, as notifications:
`{"method":"event","params":{"name":"…","extra":{…}}}`.

```
session_start     {reason, path}       reason: startup | reload | new | resume | fork
session_shutdown  {reason}             reason: quit | reload | new | resume | fork
turn_start        {prompt}
turn_end          {aborted}
tool_start        {id, name, arguments}
tool_end          {id, name, outcome, content}
compact_start     {}
compact_end       {summary}
model_change      {model}
effort_change     {effort}
```

A manifest without an `events` field is a version-1 extension and receives
`turn_end` alone.

There are no per-token events. A pipe per delta is a cost with no consumer.
`tool_end` carries the finished text.

## Requests to e

Your extension can ask e to show things, ask the user questions, and control
the session. Send `{"id":<yours>,"method":"…","params":{…}}` and read the
answer with the same id.

Every request is bounded:

- An extension can have at most 32 unanswered requests. e answers any more
  with an error.
- One modal shows at a time across all extensions.
- e sanitizes text before painting it.
- e clips oversized content rather than refusing it. A long diff still
  shows, it just ends early.

### UI requests

```
ui.notify   {message, tone?}                     → {}            tone: info | warning | error
ui.show     {title?, body, format}               → {}            a transcript block
ui.select   {title, options:[…]}                 → {value, label} | {cancelled:true}
ui.confirm  {title, message?}                    → {confirmed}
ui.input    {title, placeholder?, prefill?, secret?} → {text} | {cancelled:true}
ui.status   {text | null, key?}                  → {}            your slot on the status row (40 columns);
                                                                 `key` keeps several
ui.activity {text | null, key?}                  → {}            your text on the activity row below the
                                                                 transcript (`Thinking (3s) …`), 40 columns
ui.compose  {text}                               → {}            put text in the composer
ui.panel    {title, lines, interactive?} | null  → {}            a footer panel; null closes yours
ui.editor   {title, text?, placeholder?}         → {text} | {cancelled:true}   a multi-line answer
ui.widget   {lines | null, key?}                 → {}            rows above the composer; null removes
ui.pane     {id?, title?, side?, hint?, sections} | null → {}    a side pane; null closes yours
```

**Questions.** `select` options are strings or `{label, description?,
value?}` objects. The picker is the same one `/` opens. `confirm` is a Yes/No
picker.

`input` takes over the composer until Enter or Esc. `secret` masks the text,
and the text never reaches input hooks or the model. `editor` is the same
field for several lines. Shift+enter breaks a line, ctrl+g hands the draft to
the user's external editor, and Enter answers.

**Panels.** `panel` lines are strings, or arrays of `{text, token}` spans. e
paints each span with the theme's colour for `token`, such as `dim`,
`accent`, `success`, `warning`, `error`, or `userMessageText`. Unknown tokens
paint plain. A panel holds at most 200 lines.

Only one panel shows at a time. Another extension's panel replaces yours, and
e tells you with `ui.panel_closed`.

An `interactive` panel receives the keyboard. Every key arrives as
`{"method":"ui.key","params":{"key":"down"}}`, including chords like `ctrl+x`
and `shift+tab`. `escape` never arrives, because Esc closes the panel. ctrl+c
stays e's. To redraw, send `ui.panel` again. Your extension handles state and
key events, and e renders the frame.

**Activity.** `activity` is the row that reads `Thinking (3s) (↑1k ↓20)`
during a turn. Your text joins it through the `{activity}` token of the
user's template. See [layout](../customize/layout.md). Between turns your text
stands alone there, such as a test count, a build step, or a clock.

**Widgets and status.** `widget` rows use the same span grammar as panels and
sit above the composer. Every extension's widgets show together in key order,
eight rows at most. `{"lines": null}` removes one.

`status` with a `key` keeps several slots per extension. The status row's
template joins them with `{status}`, or picks one extension's with
`{status:<name>}`. See [layout](../customize/layout.md).

### The side pane

`ui.pane` opens a pane beside the conversation for a diff review, a plan, a
test runner, or a log. You send content. e owns the split, focus, scrolling,
the cursor, selection, and the mouse, so every pane navigates alike and none
can paint outside its column.

```json
{"id": "diff", "title": "Changes", "side": "right", "sections": [
  {"kind": "list", "id": "files", "selected": "src/main.rs",
   "items": [{"id": "src/main.rs", "label": "src/main.rs", "detail": "+12 -3"}]},
  {"kind": "diff", "id": "patch", "body": "diff --git a/src/main.rs …"}
]}
```

A pane holds these section kinds:

| Kind | Content |
| --- | --- |
| `list` | Selectable rows: `{id, label, detail?, token?}`, or plain strings. |
| `diff` | A unified diff, painted in e's row grammar. |
| `text` | Plain text. |
| `markdown` | Markdown. |
| `rows` | The panel's span lines. |

Lists show eight rows and scroll. The other kinds share the remaining height.
The whole pane holds 256 KiB. Past that, e drops the rest and the last row
says so.

To refresh, send `ui.pane` again with the same `id`. The user keeps their
place in every section that kept its `id`. Send `null` to close the pane.

What the user does comes back as notifications:

```
{"method":"pane.select",  "params":{"pane":"diff","section":"files","id":"src/main.rs"}}  the cursor moved to an item
{"method":"pane.activate","params":{"pane":"diff","section":"files","id":"src/main.rs"}}  Enter on an item
{"method":"pane.key",     "params":{"pane":"diff","key":"x"}}                               a chord e did not use
{"method":"pane.closed",  "params":{"pane":"diff"}}                                         the user closed it
```

e uses these keys while the pane has focus:

- `↑`/`↓` and `j`/`k`, `PageUp`, `PageDown`, `Home`, and `End` navigate.
- `←`/`→` scroll a wide diff.
- `Tab` moves between sections.
- `Enter` on a list activates the item and moves to the next section. On
  anything else, it attaches the selected rows to the composer as a snapshot.
- `Shift` with a movement, or a mouse drag, selects rows.
- `Esc` goes back to the first section, and then closes the pane.

The layout's focus chord, `ctrl+t` by default, moves between the conversation
and the pane. On a terminal too narrow to split, the focused one fills the
screen, and the status row says how to reach the other.

`side` is a proposal. The user's `~/.e/layout.json` decides where every pane
goes and how wide it is.

### Session requests

```
session.send      {content, internal?, run?, when?} → {}  internal: model sees it, transcript does not;
                                                         run: start (or steer) a turn — default true
                                                         for visible messages, false for internal;
                                                         when: "next_turn" holds an internal message
                                                         until the user's next prompt and sends it
                                                         just ahead of it. While a turn runs only
                                                         run: true (a steer) or when: "next_turn"
                                                         is accepted; run: false is an error then
session.info      {}                          → {path, id, name, cwd, model, effort, running,
                                                  tools, context_tokens, context_window}
session.name      {name}                      → {}
session.model     {model}                     → {} | error   the same path /model takes
session.effort    {effort}                    → {} | error   one of the model's levels
session.tools     {names | null}              → {}           narrow the toolset; null restores
session.interrupt {}                          → {}
session.compact   {focus?}                    → {}
```

`session.tools` is how a plan mode works. Send `["read","grep"]`, and the
model can neither see nor call anything else until you send `null`. It covers
built-in and extension tools alike. e enforces it at execution, not only in
what the request advertises. It resets on `/new` and resume.

### What e does not offer

These are left out on purpose:

- rewriting the provider request or its headers
- replacing the system prompt
- custom providers
- replacing the session, because `/new`, `/resume`, and `/tree` are the
  user's
- per-token streams

## Shortcuts

A shortcut binds a key chord to your extension. Declare `shortcuts` in the
manifest. When the user presses a chord, e sends
`{"id":…,"method":"shortcut","params":{"key":"ctrl+alt+g"}}`, and you answer
it like a command.

A chord needs `ctrl` or `alt`. e refuses bare keys and shift-only chords at
the manifest, because those are how text gets typed.

e keeps these chords for itself: `ctrl+c`, `ctrl+d`, `ctrl+g`, `ctrl+i`,
`ctrl+j`, `ctrl+l`, `ctrl+m`, `ctrl+o`, `ctrl+p`, `ctrl+shift+p`, `ctrl+s`,
`ctrl+v`, `ctrl+shift+v`, `ctrl+x`, and `ctrl+z`.

A chord the composer binds, such as `ctrl+k`, stays the composer's. See
[keybindings](../customize/keybindings.md). A shortcut fires only when the key
would otherwise do nothing. A user frees a chord for your extension by
unbinding it in `keybindings.json`.

When two extensions declare the same chord, the first declaration wins and e
shows a notice.

## Timeouts and failures

- The initialize answer must arrive within 5 s, or e skips the extension.
- Runtime hooks have 5 s and **fail open**. A slow or broken tool gate never
  blocks the agent. A silent `before_turn` adds nothing, and a silent
  `tool_result` changes nothing. Return `{"block":true}` to deny a tool call.
- Startup hooks are different. If an advertised startup hook errors or times
  out, launch stops. That keeps a consumed flag or branch name from leaking
  into the initial prompt.
- Your `ui.*` and `session.*` requests have no timeout, because a person
  answers them. A reload, a session switch, or shutdown answers every open
  request with an error, so you are never left waiting.
- Tool calls have 300 s. Commands have 60 s.
- On quit, e sends `shutdown`, waits a beat, then kills the process.
- e reports a crashed or missing extension in the transcript and skips it. A
  broken extension is never a reason e can't run.
- If an extension exits right after a valid initialize response, e still
  emits one notice. The notice includes which runtime hooks now fail open.

## Examples

These examples live in `docs/guides/extend/examples/`:

```
docs/guides/extend/examples/
  subagent.mjs   bounded delegated e turns as a tool, over e rpc (self-contained)
  hello.mjs      every surface at once, on the optional scaffold helper
  gate.mjs       the tool_call hook as a fail-open guard
  protected.mjs  the tool_call hook denying credential-shaped paths
  project.mjs    a startup-hook directory router (e --project <path>)
  mcp.mjs        one MCP stdio server's tools as extension tools
  scaffold.mjs   an optional wire-protocol helper (not required, never installed)
  plan.mjs       a plan mode on the new surface: session.tools, a shortcut, a pane,
                 ui.select, a status slot, and a panel
```

An extension speaks the protocol directly. `subagent.mjs` and the shell
`ping.sh` below are single self-contained files. Each reads a JSON request
per line and writes a response per line. e installs nothing beside an
extension.

- **`hello.mjs`.** Every surface at once, on the optional scaffold: a
  command, a tool, config, an input hook, and session naming, in about 50
  lines of handlers.
- **`gate.mjs`.** The `tool_call` hook as a guard, in e's fail-open shape.
  Only an explicit block stops a call. A slow or crashed extension never
  blocks the agent.
- **`protected.mjs`.** The `tool_call` hook denies any call to `read`,
  `write`, `edit`, `grep`, or `bash` that touches a credential-shaped path:
  `~/.ssh`, `~/.aws`, `~/.gnupg`, `.env*`, `*.pem`, or `*.key`. It catches the
  tool's `path` argument and bash commands that mention one. `gate.mjs` denies
  destructive commands. This one is about what gets read into context or
  written to disk, not just what bash runs. See
  [sandboxing](../usage/sandboxing.md) for e's trust model and where a hook
  like this fits.
- **`project.mjs`.** This startup-hook launcher uses the scaffold.
  `e --project <path>` relaunches e in an existing project directory.
- **`subagent.mjs`.** Its `delegate` tool drives a single-shot `e rpc
  --no-extensions` child with one JSON request line in and one result out. The
  delegated turn is extension-free, so it cannot delegate again. It defines
  `Explore`, `Plan`, and `Build` in the extension and sends each agent's
  `tools` and optional `model` in the RPC request. Core stays generic and does
  not have an agent type.
- **`mcp.mjs`.** A dependency-free bridge from one configured MCP stdio
  server's `tools/list` and `tools/call` surface into e extension tools. It
  forwards MCP progress through the additive `tool.update` capability.

### The scaffold helper

`scaffold.mjs` is an optional convenience. It handles the same stdin and
stdout framing and the id routing, and it provides a
`connect({ manifest, handlers })` wrapper. You write handlers instead of a
read loop.

To use it, drop it into your extension's own bundle directory and
`import { connect } from "./scaffold.mjs"`. e never installs it for you, and
you don't need to think about it otherwise.

### Use or share an example

To use an example, put it in `~/.e/extensions/`, make it executable, and
restart e. It can be a top-level executable file, or a subdirectory bundling
it and its helpers.

An example that uses the scaffold helper needs `scaffold.mjs` beside it in its
bundle. The self-contained ones, `subagent.mjs` and `ping.sh`, need nothing.

To share an extension, put it in a repository's `extensions/` directory.
Others install it with `e install git:<host>/<user>/<repo>`. See
[packages.md](packages.md).

## Compiled extensions

Extensions are programs, not only scripts. Anything that speaks the line
protocol qualifies, including a compiled binary.

A compiled extension lives in its own repository, like every package. It
reaches users as a release package with
`e install release:<owner>/<repo>/<name>`. See [packages.md](packages.md). The
e repository ships no extensions of its own.

What an extension sends still crosses the line as data. e sanitizes notices
before painting them. An extension that wants colour returns a `show` with a
`format` rather than styled bytes.

## A complete extension in shell

Save this as `~/.e/extensions/ping.sh` and make it executable with
`chmod +x`:

```sh
#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"initialize"'*)
      printf '{"id":%s,"result":{"name":"ping","version":"1","commands":[{"name":"ping","description":"are you there"}]}}\n' "$id" ;;
    *'"command"'*)
      printf '{"id":%s,"result":{"notice":"pong"}}\n' "$id" ;;
    *'"shutdown"'*) exit 0 ;;
  esac
done
```

Restart e, type `/ping`, and you get `pong`.

## MCP tools

`mcp.mjs` exposes one MCP stdio server's tools as e tools. Put it in
`~/.e/extensions/` and make it executable. Then configure the stdio server
that e should own in `~/.e/settings.json`:

```json
{
  "extensions": {
    "mcp": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/safe/root"]
    }
  }
}
```

> [!TIP]
> `npx -y` downloads the server on first use. That routinely takes longer
> than the 5 s initialize budget, so e skips the bridge with
> `initialize timed out` until the package is cached. Run the `npx` line once
> by hand first, or point `command` at an installed binary.

The bridge intentionally maps only MCP tools. Prompts, resources, sampling,
elicitation, and authorization stay out of e's core and out of this example.

The bridge uses:

- the 2025-11-25 initialize/initialized stdio lifecycle supported by current
  SDK legacy/default mode
- newline-delimited JSON-RPC
- paginated `tools/list`
- `tools/call`

See the [MCP lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle),
[transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
and [tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
specifications.

## Delegated turns

`subagent.mjs` gives the model a `delegate` tool that runs a task in a child
e. Put it in `~/.e/extensions/` and restart. It is a single self-contained
file with nothing beside it.

Each delegation is a single-shot `e rpc` child in the same working directory.
The extension writes one JSON request line, reads one result object, and
closes stdin. The child loads no extensions, so it cannot delegate again. Set
`E_BIN` when the child should use an e binary other than the one on `PATH`.

`timeout_seconds` defaults to 240 seconds. At the deadline the extension
sends SIGTERM, and `e rpc` kills every active built-in bash process group
before it exits. A later SIGKILL remains as a watchdog if graceful shutdown
stalls.

The delegation sets `save: true`. The response includes the saved JSONL path,
and the tool result gives that path to the parent. The parent can read it when
the final answer omits a useful tool call or result.

### Agents live in the extension

A delegation can name an `agent` defined in `subagent.mjs`. Each agent
chooses a built-in tool allowlist and an optional model. The child receives
the task as its user message. It uses e's normal system prompt with a generic
tool-policy suffix. Core does not have an agent type.

The extension defines these agents:

- `Explore` can use `read` and `grep`.
- `Plan` can use `read` and `grep`.
- `Build` can use every built-in tool.

Each agent object has `name`, `description`, optional `tools`, and optional
`model`. The shipped `"{provider/model}"` values are placeholders. Until you
replace one, that child uses the model `e rpc` normally resolves from
configuration. A call can also pass `model` to override the selected agent.

The core validates `tools` as built-in names and advertises only those
schemas. It enforces the same list when a provider emits a tool call.

## What startup hooks are for

A startup extension sees raw argv and can relaunch the same binary in a new
cwd. That lets it implement project-directory routing with `--project <path>`,
project profiles, or scratch-directory routing. Any language that speaks the
line protocol can add these behaviors without hardcoding them in e.
