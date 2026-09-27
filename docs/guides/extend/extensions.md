---
title: Extensions
description: Add tools, commands, hooks, and UI over a JSON line protocol.
order: 1
---

# Extensions

An extension is a program, in any language, that adds tools, commands,
hooks, and UI to e. This guide starts with a working extension and then
documents the whole protocol.

e runs each extension as its own process and exchanges one JSON object per
line with it over stdin and stdout. An extension never touches the
terminal: what it shows is data that e paints through the user's theme.

## Quick start

This extension registers one tool, `greet`, that the model can call. Save it
as `greet.mjs`:

```js
#!/usr/bin/env node
// greet: one tool the model can call.
import { createInterface } from "node:readline";

const send = (message) => process.stdout.write(JSON.stringify(message) + "\n");

const manifest = {
  name: "greet",
  version: "0.1",
  tools: [
    {
      name: "greet",
      description: "Greet someone by name.",
      parameters: {
        type: "object",
        properties: { who: { type: "string" } },
        required: ["who"],
      },
      label: { running: "Greeting", completed: "Greeted", target: "who" },
    },
  ],
};

createInterface({ input: process.stdin }).on("line", (line) => {
  const { id, method, params } = JSON.parse(line);
  if (method === "initialize") send({ id, result: manifest });
  else if (method === "tool_call") send({ id, result: { content: `Hello, ${params.arguments.who}!` } });
  else if (method === "shutdown") process.exit(0);
  else if (id !== undefined) send({ id, error: `unsupported method ${method}` });
});
```

Install it and make it executable:

```sh
mkdir -p ~/.e/extensions
cp greet.mjs ~/.e/extensions/
chmod +x ~/.e/extensions/greet.mjs
```

`~/.e` is e's home directory; [settings](../customize/settings.md) says
where your build keeps it. Start e, or run `/reload`, and ask
`Use the greet tool to greet Ada.` The transcript shows `Greeting Ada`, then
`Greeted Ada`, and the model reads `Hello, Ada!`.

If the extension fails to start, the transcript says why
(`extension greet.mjs: …`). Each line it writes to stderr shows there too,
so use stderr for debug output. `e --no-extensions` starts e without any
extensions.

## Where extensions load from

e starts every extension it finds, in this order:

1. `~/.e/extensions/`, sorted by path.
2. The `extensions/` directory of each [package](packages.md): the
   `packages` list in settings, then a trusted repository's `.e/packages`
   list, then `e --package` for this run.

This is the **extension order**. Hooks run in it, and when two extensions
declare the same tool or shortcut, the first wins and e shows a notice.
Extensions never load from a repository's own `.e/extensions/`, even a
trusted one.

An extension is a top-level executable file, such as `greet.mjs`, or a
directory that bundles an entry point with helper files. In a directory, e
runs the first executable it finds, checking in this order (ties go to the
first sorted path):

1. a file named `index.*`
2. a file named after the directory, such as `foo/foo.mjs`
3. the only executable in the directory

On Unix, files without the executable bit never start, so helpers beside
an entry point are safe.

### Configuration

Keep an extension's settings in `~/.e/settings.json` under
`"extensions"`, keyed by its manifest name, such as
`{"extensions": {"greet": {"greeting": "Hi"}}}`. e passes the whole
`"extensions"` object to every extension as `extensions_config` in the
`initialize` params.

## Protocol overview

The protocol is version 1. Newer features are additive families listed in
`initialize` as `capabilities`; an extension may ignore them all.

- **Framing:** one UTF-8 JSON object per line. A stdout line over 1 MiB ends
  the extension.
- **Requests** carry `id` and `method`, and get exactly one answer with the
  same `id`: `{"id":…,"result":{…}}` or `{"id":…,"error":"message"}`.
- **Notifications** carry `method` and no `id`, and get no answer.
- **Both sides send requests.** e's ids are integers; yours may be any JSON
  value. A line with both `id` and `method` is always a request, so the id
  spaces never collide.

### Messages from e

| Method | Kind | Params | Answer |
| --- | --- | --- | --- |
| `initialize` | request | `{protocol, capabilities, ui, e_version, cwd, extensions_config}` | [the manifest](#initialize) |
| `hook.startup` | request | `{cwd, argv, flags}` | [`hook.startup`](#hookstartup) |
| `tool_call` | request | `{name, arguments}` | [`tool_call`](#tool_call) |
| `command` | request | `{name, args}` | [`command`](#command) |
| `command.complete` | request | `{name, prefix}` | [`command.complete`](#commandcomplete) |
| `shortcut` | request | `{key}` | same as `command` |
| `hook.tool_call` | request | `{name, arguments}` | [`hook.tool_call`](#hooktool_call) |
| `hook.input` | request | `{text}` | [`hook.input`](#hookinput) |
| `hook.before_turn` | request | `{prompt}` | [`hook.before_turn`](#hookbefore_turn) |
| `hook.tool_result` | request | `{name, content, is_error}` | [`hook.tool_result`](#hooktool_result) |
| `hook.render` | request | `{kind, name, content}` | [`hook.render`](#hookrender) |
| `hook.compact_summary` | request | `{summary}` | [`hook.compact_summary`](#hookcompact_summary) |
| `flags` | notification | `{flags}` | [Flags](#flags) |
| `event` | notification | `{name, extra}` | [Events](#events) |
| `ui.key` | notification | `{key}` | a key while your interactive panel is open |
| `ui.panel_closed` | notification | `{}` | your panel was closed |
| `pane.select` | notification | `{pane, section, id}` | the side pane's cursor moved to an item |
| `pane.activate` | notification | `{pane, section, id}` | Enter on a pane item |
| `pane.key` | notification | `{pane, key}` | a pane chord e did not use |
| `pane.closed` | notification | `{pane}` | your pane was closed |
| `shutdown` | notification | none | exit; e kills the process 150 ms later |

The `initialize` params look like this:

```json
{"id":1,"method":"initialize","params":{"protocol":1,"capabilities":["tool.update","events","hooks","display","ui","session","shortcuts","pane","widget","render"],"ui":true,"e_version":"0.0.2","cwd":"/path/to/project","extensions_config":{"greet":{"greeting":"Hi"}}}}
```

### Messages from an extension

```json
{"id":1,"result":{"name":"greet"}}
{"id":2,"error":"what went wrong"}
{"method":"notify","params":{"message":"a transcript notice, at any time"}}
{"method":"tool.update","params":{"id":3,"stream":"stdout","chunk":"working\n"}}
{"id":"q1","method":"ui.select","params":{"title":"Pick one","options":["a","b"]}}
```

The last line is a [request to e](#requests-to-e).

### Which frontends support what

e has three frontends that can run extensions. `ui` in the `initialize`
params is true when someone can answer `ui.*` requests.

| Feature | Terminal | `e rpc` | `e -p` | [SDK](sdk.md) with `extensions(true)` |
| --- | --- | --- | --- | --- |
| `initialize` `ui` | true | true | false | false |
| Tools, `tool_call`, `before_turn`, `tool_result`, `compact_summary` hooks | yes | yes | yes | yes |
| `hook.startup` and `flags` | yes | yes | yes | no |
| Commands, shortcuts, `input` and `render` hooks | yes | no | no | no |
| `turn_*`, `tool_*`, `compact_*` events | yes | yes | yes | yes |
| `session_*`, `model_change`, `effort_change` events | yes | no | no | no |
| `ui.*` and `session.*` requests | yes | see below | `no ui` error | `no ui` error |

Under `e rpc`, `ui.notify` and `ui.show` reach the client as `notice` lines
and succeed. `ui.select`, `ui.confirm`, `ui.input`, and `ui.editor` reach
the client as `ask` lines when it opted in, and fail with `no ui` otherwise.
Every other `ui.*` and `session.*` request fails with `no ui`. See
[automation](../usage/automation.md). Handle the error in every case.

## Results by method

Each request from e expects a result of a specific shape. A result that
does not parse counts as a failure: a runtime hook then changes nothing, a
startup hook stops launch, and a tool or command reports an error.

### `initialize`

Your answer to `initialize` is the manifest. Only `name` is required;
unknown fields are ignored.

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
  "commands": [{"name": "deploy", "description": "deploy an environment", "arguments": "<env>", "completions": true}],
  "flags": [
    {"name": "project", "type": "string", "description": "relaunch in this directory"},
    {"name": "plan", "type": "boolean", "description": "plan mode", "default": false}
  ],
  "hooks": ["startup", "tool_call", "input", "before_turn", "tool_result", "compact_summary", "render"],
  "renders": ["tool:bash", "assistant"],
  "events": ["session_start", "turn_start", "tool_end"],
  "shortcuts": [{"key": "ctrl+alt+g", "description": "greet"}]
}
```

`name` identifies the extension in notices, settings, and status slots.
`hooks` lists the hooks e should call; each is described below. A tool's
`parameters` is a JSON Schema object (anything else becomes an empty object
schema), and a tool with a built-in's name replaces that built-in. A tool's
`label` gives its transcript row the built-in grammar:

| Label field | Meaning |
| --- | --- |
| `category` | The noun used when e tallies a batch of tool calls. |
| `running`, `completed` | Verbs, shown as given. |
| `target` | The argument whose value the row shows. |

Without a label, the row reads `Running greet` and then `Ran greet`.

### Flags

Declared `flags` appear in `e --help` and `/help`. `type` is `"boolean"`
(the default) or `"string"`. e parses a flag whose `name` uses only
letters, digits, and `-`:

- A boolean matches `--name`, `--no-name`, and `--name=<value>`, where `1`,
  `true`, `yes`, and `on` mean true and anything else false.
- A string matches `--name=value` and `--name value`. A following token that
  starts with `-` is never taken as the value; the flag then parses as
  `null`, as it does at the end of argv.
- The last occurrence wins, and `--` stops parsing.

Any other name, such as `"-x, --example"`, is help text only; read it from
the startup hook's raw `argv`. At launch, before any startup hook, e sends
the parsed values to every extension that declares a parsable flag:

```json
{"method":"flags","params":{"flags":{"project":"../app","plan":true}}}
```

Only flags actually passed appear, so "passed false" differs from "not
passed". e never fills in a declared `default`; apply it yourself. After the
startup hooks, e removes parsed flags and their values from argv before it
reads its own arguments and the initial prompt.

### `tool_call`

Your answer is the tool's result:

```json
{"content":"text the model sees","is_error":false,"summary":"+12 -3","display":"…","format":"diff","session_name":"optional new session name"}
```

| Field | Meaning |
| --- | --- |
| `content` | Everything the model reads. |
| `is_error` | Marks the result as a failure. |
| `summary` | The suffix on the tool's transcript row. |
| `display` | What the ctrl+o viewer shows instead of `content`, up to 64 KiB. Never reaches the model. |
| `format` | How the viewer paints it: `text` (default), `markdown`, or `diff`. With `diff`, e takes a unified diff (`git diff` output) and paints it with line numbers and coloured markers. Unknown values read as `text`. |
| `session_name` | Renames the session, as shown in `/resume`. |

A timeout, crash, or unparsable result becomes an error result for the
model.

To stream progress before the result, send `tool.update` notifications:

```json
{"method":"tool.update","params":{"id":3,"stream":"stdout","chunk":"working\n"}}
```

`id` is the `tool_call` request's id, and `stream` is `stdout` or `stderr`.
e shows the chunks in order in the same tool-output stream as built-in
commands. Updates are optional.

### `command`

Your answer to `command` may combine these fields:

| Field | Effect |
| --- | --- |
| `notice` | A line in the transcript. |
| `show` | A transcript block, `{"title":"…","body":"…","format":"markdown"}`; `body` is clipped at 64 KiB. |
| `prompt` | Text submitted as if the user typed it. |
| `session_name` | Renames the session. |

`params.args` is the rest of the line after `/name`. A failure shows as a
notice.

A command that declares `"arguments": "<env>"` shows that hint in the `/`
picker, and picking it leaves `/name ` in the composer for the user to
finish.

### `command.complete`

A command that declares `"completions": true` is asked for argument
completions as the user types `/name pre`:

```json
{"id":11,"method":"command.complete","params":{"name":"deploy","prefix":"st"}}
```

Answer `{"items":[{"value":"staging","label":"staging","description":"the staging cluster"}]}`.
`label` and `description` are optional. e opens a picker of up to 50 items,
and the chosen `value` replaces the prefix. A slow, failed, or empty answer
shows nothing.

### `hook.startup`

Rewrite the command line or relaunch e elsewhere. Startup hooks run in
extension order before e parses its subcommands, `-c`, `-r`, or the initial
prompt. The params are `{cwd, argv, flags}`. Answer, with every field
optional:

```json
{"argv":["-c"],
 "env":{"REMOVE_ME":null},
 "relaunch":{"cwd":"/path/to/project","env":{"BOOTSTRAPPED":"1"}}}
```

- `argv` replaces the arguments the next hook, and then e, sees.
- `env` sets variables in the current process; `null` removes one.
- `relaunch` replaces the process with the same e binary in `cwd`, with
  `env` applied, and ends the chain. No other executable can be chosen.

Unlike other hooks, startup hooks fail closed. If one errors, times out, or
returns an invalid result, e prints the error and exits with status 1, so a
consumed flag or branch name never leaks into the prompt.

### `hook.tool_call`

Gate a tool call before it runs. Answer `{"block":true,"reason":"why"}` to
stop it; the model sees the reason as an error result (`blocked by <name>`
without one). Answer `{"block":false}` or `{}` to allow it. The first
extension to block wins.

### `hook.input`

Decide what happens to a line the user submits. e runs the hook in
extension order, and the first extension to consume or replace the line
wins:

```json
{"consume":true,"notice":"swallowed, with a notice"}
{"replace":"the rewritten line"}
{"notice":"let through, with a notice"}
{}
```

An empty result lets the line through untouched. Every notice is shown,
including those from extensions that let the line through. A pasted API key
never reaches the hook.

### `hook.before_turn`

Add context to a turn. e runs it once per turn, before the first provider
request, with the prompt that started the turn. Answer:

```json
{"system_suffix":"a paragraph appended to the system prompt for this turn","message":{"content":"…","internal":true}}
```

e appends `system_suffix` to the system prompt; it never replaces the
prompt. e adds `message` to the conversation before the request. `internal`
defaults to true, which keeps the message out of the transcript.

### `hook.tool_result`

Redact or trim tool output. e runs it after every tool, before the result
is shown, stored, or sent. Answer `{"content":"what the model reads instead"}`,
or `{}` to keep the result. Each extension sees the previous one's rewrite.
A rewrite also drops the tool's `display` text, so the viewer shows exactly
what you let through.

### `hook.render`

Replace how an entry is displayed. Add `render` to `hooks` and list what to
render in `renders`:

- `"tool:<name>"`, or `"tool:*"` for every tool, asks about a tool's
  finished result. Your body replaces it in the ctrl+o viewer.
- `"assistant"` asks about a completed reply. Your body replaces its
  markdown in the transcript.
- `"*"` asks about both.

The params are `{"kind":"tool"|"assistant","name":"…","content":"…"}`;
`name` is empty for a reply. Answer `{"body":"…","format":"text"|"markdown"|"diff"}`,
or `{}` to leave the entry as e paints it. Each extension sees the previous
one's body. The model never sees the rendered body.

### `hook.compact_summary`

Edit the summary that is about to replace the older conversation during
compaction. Answer `{"summary":"…"}` or `{}`. Each extension sees the
previous one's summary.

## Events

List the events you want in the manifest's `events`. e sends them as
notifications:

```json
{"method":"event","params":{"name":"turn_end","extra":{"aborted":false}}}
```

| Event | `extra` |
| --- | --- |
| `session_start` | `{reason, path}`, where `reason` is `startup`, `reload`, `new`, `resume`, or `fork` |
| `session_shutdown` | `{reason}`: `quit`, `reload`, `new`, `resume`, or `fork` |
| `turn_start` | `{prompt}` |
| `turn_end` | `{aborted}` |
| `tool_start` | `{id, name, arguments}` |
| `tool_end` | `{id, name, outcome, content}`, where `outcome` is `completed`, `failed`, `timedout`, `blocked`, or `cancelled` |
| `compact_start` | `{}` |
| `compact_end` | `{summary}` |
| `model_change` | `{model}` |
| `effort_change` | `{effort}` |

A manifest without an `events` field receives `turn_end` only. Unknown
event names are ignored. There are no per-token events; `tool_end` carries
the finished text.

## Requests to e

An extension can ask e to show things, ask the user questions, and control
the session. Send `{"id":<yours>,"method":"…","params":{…}}` and read the
answer with the same id. Unknown methods fail with `unknown method <name>`.

Every request is bounded:

- An extension can have at most 32 unanswered requests; more fail at once.
- One modal (`select`, `confirm`, `input`, `editor`) shows at a time across
  all extensions; the rest queue.
- e sanitizes text before painting it and clips oversized content rather
  than refusing it.
- Requests have no timeout, because a person answers them; see
  [Timeouts and failures](#timeouts-and-failures).

### UI requests

| Method | Params | Answer |
| --- | --- | --- |
| `ui.notify` | `{message, tone?}`, tone `info` (default), `warning`, or `error` | `{}` |
| `ui.show` | `{title?, body, format?}` | `{}`; a transcript block like a command's `show` |
| `ui.select` | `{title, options}` | `{value, label}` or `{cancelled:true}` |
| `ui.confirm` | `{title, message?}` | `{confirmed}` |
| `ui.input` | `{title, placeholder?, prefill?, secret?}` | `{text}` or `{cancelled:true}` |
| `ui.editor` | `{title, text?, placeholder?}` | `{text}` or `{cancelled:true}` |
| `ui.compose` | `{text}` | `{}`; replaces the composer draft |
| `ui.status` | `{text, key?}` | `{}`; `text: null` clears the slot |
| `ui.activity` | `{text, key?}` | `{}`; `text: null` clears the slot |
| `ui.widget` | `{lines, key?}` | `{}`; `lines: null` removes the widget |
| `ui.panel` | `{title?, lines, interactive?}`, or `null` | `{}`; `null` closes your panel |
| `ui.pane` | `{id?, title?, side?, hint?, sections}`, or `null` | `{}`; `null` closes your pane. See [The side pane](#the-side-pane). |

**Questions.** `select` options are strings or `{label, description?,
value?}` objects (`value` defaults to the label); at least one is required.
`confirm` is a Yes/No picker; Esc answers it `{confirmed:false}` and cancels
the others. `input` turns the composer into the answer field; `secret`
masks the text and keeps it from input hooks and the model. `editor` is the
multi-line version: Shift+Enter breaks a line and Ctrl+G opens the user's
external editor. `ui.compose` fails while the composer is answering.

**Spans.** Panel, widget, and `rows` pane lines are strings or arrays of
`{text, token}` spans, painted in the theme's colour for `token` (`dim`,
`accent`, `success`, `warning`, `error`, `userMessageText`, …). Unknown
tokens paint plain.

**Panels.** A footer surface of up to 200 lines, titled with the extension
name by default. One panel shows at a time; when another replaces yours, e
sends `ui.panel_closed`. An `interactive` panel receives every key as a
`ui.key` notification, such as `{"key":"shift+tab"}`, except Esc (closes the
panel) and Ctrl+C. To redraw, send `ui.panel` again.

**Status and activity.** `status` is a slot on the status row; `activity` is
text on the row below the transcript that reads `Thinking (3s) …` during a
turn. Each is one line of at most 40 columns, and a `key` keeps several per
extension. The user's [layout](../customize/layout.md) places them with
`{status}` (all slots), `{status:<name>}` (one extension's), and
`{activity}`.

**Widgets.** Rows above the composer. All extensions' widgets show
together, ordered by extension name and key, eight rows at most.

### The side pane

`ui.pane` opens a pane beside the conversation for a diff review, a plan,
or a log. You send content; e owns the layout, focus, scrolling, selection,
and the mouse.

```json
{"id": "diff", "title": "Changes", "side": "right", "sections": [
  {"kind": "list", "id": "files", "selected": "src/main.rs",
   "items": [{"id": "src/main.rs", "label": "src/main.rs", "detail": "+12 -3"}]},
  {"kind": "diff", "id": "patch", "body": "diff --git a/src/main.rs …"}
]}
```

| Pane field | Meaning |
| --- | --- |
| `id` | Identifies the pane; defaults to the extension name. |
| `title` | Defaults to the `id`. |
| `side` | `left` or `right`. A proposal: the user's `~/.e/layout.json` decides where every pane goes and how wide it is. |
| `hint` | Replaces the pane's default key hint. |
| `sections` | At least one section. |

Each section has a `kind`, an optional `id` (defaults to its position), and
an optional `title`:

| Kind | Content |
| --- | --- |
| `list` | `items`: `{id, label, detail?, token?}` objects or plain strings. `selected` names the item under the cursor. Shows eight rows and scrolls. |
| `diff` | `body`: a unified diff, painted in e's diff rows. |
| `text` | `body`: plain text. |
| `markdown` | `body`: markdown. |
| `rows` | `lines`: span lines, as in a panel. |

A pane holds 256 KiB; e drops the rest and says so on the last row. To
refresh, send `ui.pane` again with the same `id`; the user keeps their
place in every section whose `id` survived. One pane shows at a time; when
another replaces yours, e sends `pane.closed`.

What the user does comes back as `pane.select`, `pane.activate`,
`pane.key`, and `pane.closed` notifications, listed in
[Messages from e](#messages-from-e). e uses these keys while the pane has
focus:

- Up/Down, `j`/`k`, PageUp, PageDown, Home, and End navigate; Left/Right
  scroll a wide diff; Tab moves between sections.
- Enter on a list activates the item and moves to the next section;
  elsewhere it attaches the selected rows to the composer.
- Shift with a movement key, or a mouse drag, selects rows.
- Esc returns to the first section, then closes the pane.

Other chords arrive as `pane.key`. The layout's focus chord (`ctrl+t` by
default) moves between conversation and pane; on a terminal too narrow to
split, the focused one fills the screen.

### Session requests

| Method | Params | Answer |
| --- | --- | --- |
| `session.send` | `{content, internal?, run?, when?}` | `{}` |
| `session.info` | `{}` | `{path, id, name, cwd, model, effort, running, tools, context_tokens, context_window}` |
| `session.name` | `{name}` | `{}`; an empty name clears it |
| `session.model` | `{model}` | `{}`, or an error; resolves and saves the model the way `/model` does |
| `session.effort` | `{effort}` | `{}`, or an error; one of the current model's levels |
| `session.tools` | `{names}` | `{}`; a list narrows the toolset, `null` restores it |
| `session.interrupt` | `{}` | `{}`; stops a running turn |
| `session.compact` | `{focus?}` | `{}`; compacts now, optionally with a focus |

`session.send` puts a message into the session:

- `internal: true` means the model sees it and the transcript does not.
- `run` starts a turn, or steers the running one. It defaults to true for a
  visible message and false for an internal one.
- `when: "next_turn"` holds an internal message and sends it just ahead of
  the user's next prompt. It requires `internal: true`.
- While a turn runs, only `run: true` or `when: "next_turn"` is accepted.
  During compaction or a reload, every send fails; try again later.

`session.tools` is how a plan mode works. Send `{"names":["read","grep"]}`,
and the model can neither see nor call anything else until you send
`{"names":null}`. It covers built-in and extension tools, is enforced at
execution, and resets on `/new` and resume.

### What e does not offer

These are left out on purpose:

- rewriting the provider request or its headers
- replacing the system prompt
- custom providers
- replacing the session: `/new`, `/resume`, and `/tree` are the user's
- per-token streams

## Shortcuts

A shortcut binds a key chord to your extension. When the user presses a
declared chord, e sends `{"id":…,"method":"shortcut","params":{"key":"ctrl+alt+g"}}`,
and you answer it like a [command](#command).

A chord needs `ctrl` or `alt`. e refuses bare keys and shift-only chords,
because those type text, and keeps these for itself: `ctrl+c`, `ctrl+d`,
`ctrl+g`, `ctrl+i`, `ctrl+j`, `ctrl+l`, `ctrl+m`, `ctrl+o`, `ctrl+p`,
`ctrl+shift+p`, `ctrl+s`, `ctrl+v`, `ctrl+shift+v`, `ctrl+x`, and `ctrl+z`.
A refused chord is dropped with a notice.

A chord the composer binds, such as `ctrl+k`, stays the composer's: a
shortcut fires only when the key would otherwise do nothing. A user frees a
chord by unbinding it in `keybindings.json`; see
[keybindings](../customize/keybindings.md).

## Timeouts and failures

| Request | Budget | On timeout or failure |
| --- | --- | --- |
| `initialize` | 5 s | The extension is skipped, with a notice. |
| `hook.startup` | 5 s | Launch stops; e exits with status 1. |
| Other hooks | 5 s | Fail open: the hook changes nothing and blocks nothing. |
| `tool_call` | 300 s | The model gets an error result. |
| `command`, `shortcut` | 60 s | A notice. |
| `command.complete` | 3 s | No completions. |
| Your `ui.*` and `session.*` requests | none | Failed with an error on reload, session switch, or shutdown. |

Runtime hooks fail open, so a slow or broken gate never blocks the agent.
Only an explicit `{"block":true}` denies a tool call.

A crashed, missing, or misbehaving extension is reported in the transcript
and skipped; it never stops e from running. If an extension exits
mid-session, e shows one notice, which names any `tool_call` or `input`
hook that no longer applies.

## Examples

These examples live in [`examples/`](examples/README.md):

| File | What it shows |
| --- | --- |
| [`hello.mjs`](examples/hello.mjs) | A command, a tool, config, an input hook, and session naming, on the scaffold. |
| [`plan.mjs`](examples/plan.mjs) | A plan mode: `session.tools`, a `before_turn` paragraph, a shortcut, a picker, a status slot, and a side pane, on the scaffold. |
| [`gate.mjs`](examples/gate.mjs) | The `tool_call` hook as a fail-open guard against destructive commands. |
| [`protected.mjs`](examples/protected.mjs) | The `tool_call` hook denying calls that touch credential-shaped paths such as `~/.ssh` or `.env`. See [sandboxing](../usage/sandboxing.md) for where a hook like this fits. |
| [`project.mjs`](examples/project.mjs) | A startup hook that adds `e --project <path>` and relaunches e in that directory, on the scaffold. |
| [`mcp.mjs`](examples/mcp.mjs) | One MCP stdio server's tools as e tools. Self-contained. |
| [`subagent.mjs`](examples/subagent.mjs) | A `delegate` tool that runs one turn in a child `e rpc --no-extensions`, with agents (`Explore`, `Plan`, `Build`) defined as tool allowlists plus a model. Replace its `"{provider/model}"` placeholders; `E_BIN` picks the e binary. Self-contained. |
| [`scaffold.mjs`](examples/scaffold.mjs) | An optional helper for the protocol. |

To try one, copy it into `~/.e/extensions/`, make it executable, and
restart e. An example built on the scaffold needs `scaffold.mjs` beside it
in a bundle directory:

```sh
mkdir -p ~/.e/extensions/hello
cp docs/guides/extend/examples/hello.mjs docs/guides/extend/examples/scaffold.mjs ~/.e/extensions/hello/
chmod +x ~/.e/extensions/hello/hello.mjs
```

To share an extension, including a compiled one, put it in a
[package](packages.md).

### The scaffold helper

`scaffold.mjs` handles framing and id routing for Node.js extensions: pass
a manifest and handlers to `connect()` and call `.run()`. Its header comment
lists every handler. Tool handlers get an `{update}` argument that sends
`tool.update` chunks, and `flag(name)` returns a flag's value or its
declared default. Copy it into your bundle; e never installs it.

### MCP tools

`mcp.mjs` exposes one MCP stdio server's tools as e tools (MCP 2025-11-25
stdio; tools only, not prompts, resources, sampling, or elicitation).
Configure the server it starts in `~/.e/settings.json`:

```json
{"extensions": {"mcp": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/safe/root"]}}}
```

> [!TIP]
> `npx -y` downloads the server on first use, which often takes longer than
> the 5-second `initialize` budget: e then skips the bridge with
> `initialize timed out`. Run the `npx` command once by hand first, or point
> `command` at an installed binary.
