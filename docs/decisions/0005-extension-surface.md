# The extension surface: pi's reach across a process boundary

Status: accepted
Date: 2026-09-13

## Context

Version 1 of the extension protocol gave a child process five things: tools,
slash commands, three hooks (startup, tool_call, input), one event
(turn_end), and a plain-text notice. That is enough to gate and to add
tools; it is not enough to build the things people actually build for a
coding agent: a diff view, a todo overlay, a question the extension asks
the user, a status readout, a per-turn summary with structure, a message
injected into context without starting a turn.

pi, the reference for this comparison, runs extensions in process and hands
them the whole program: about forty lifecycle events, hooks that rewrite the
provider request and the system prompt, `ctx.ui` with selects, confirms,
inputs, widgets, overlays and custom components that receive keystrokes,
message renderers for transcript entries, session control (new, fork,
switch, navigate), model selection, custom providers, and a message API.
That reach is the reason pi has a package ecosystem.

e chose a process boundary on purpose (decision 0003, `docs/architecture.md`):
the harness stays small, extensions run in any language, a crash is a
notice rather than a panic. The question is whether the reach can cross the
boundary without giving up what the boundary buys.

## Decision

It can, under one rule: **what crosses the line is data, never code and
never terminal bytes.** Extensions describe; e paints, through the theme.
Concretely, protocol 1 grows by five additive families, each advertised as
an initialize capability and each declared in the manifest so a version-1
extension is never sent a message it cannot answer.

### 1. Events — subscribe to the lifecycle

The manifest lists `events`; e sends only those, as notifications. A
version-1 manifest (no `events` field) keeps receiving `turn_end` alone.

```
session_start {reason, path}       session_shutdown {reason}
turn_start {prompt}                turn_end {aborted}
tool_start {id, name, arguments}   tool_end {id, name, outcome, content}
compact_start {}                   compact_end {summary}
model_change {model}               effort_change {effort}
```

`reason` is `startup`, `reload`, `new`, or `resume` — pi's vocabulary, so a
package written against pi's lifecycle ports by renaming. Not offered:
streaming deltas (`message_update`). A pipe per token is a cost with no
consumer; `tool_end` carries the finished text.

### 2. Hooks — bounded, fail-open, additive

Declared in `hooks`, five seconds, and every failure resolves to "continue".

```
startup, tool_call, input          unchanged
before_turn {prompt}               → {system_suffix?, message?}
tool_result {name, content, is_error} → {content?}
compact_summary {summary}          → {summary?}
```

`before_turn` appends to the system prompt; it cannot replace it. That is
the whole difference from pi's `before_agent_start`, and it is deliberate:
the system prompt is the user's file-backed contract with the model, and an
extension gets a paragraph, not the page. `tool_result` is pi's
`tool_result`, for redaction and trimming. `compact_summary` edits the
generated summary; full custom compaction (pi's `session_before_compact`
returning its own summary) is not offered because it would ship the whole
history over the pipe on every compaction.

Not offered: `before_provider_request`, `before_provider_headers`, and
`context`. Rewriting the outbound payload or its headers means the wire
dialects stop being the only thing that talks to a provider, and the guard's
promise about where user data goes stops being checkable. `context` would
ship the whole history over the pipe before every request.

One deliberate difference from pi: pi's `tool_call` gate is fail-closed (a
handler that throws blocks the tool). e's stays fail-open, as decision 0003
and `docs/sandboxing.md` state — a hook is a speed bump, not the boundary,
and a crashed gate must not make the agent unusable. An extension that
wants fail-closed answers `{"block": true}` itself when it is unsure.

### 3. Display — declarative rendering, painted by e

pi hands extensions a render function. e hands them a vocabulary.

- A tool declaration may carry `label: {category, running, completed,
  target}` where `target` names the argument the row shows. The tool then
  wears a row like a built-in's (`Diffing src/main.rs`, `Diffed
  src/main.rs`) instead of `Running diff`.
- A tool result may carry `summary` (the row's suffix), `display` (viewer
  detail, as the built-in edit does), and `format`.
- A command result may carry `show`.
- `show` is also an extension-initiated request, any time.

`show` and `display` share one body contract: `{title?, body, format}` with
`format` one of `text`, `markdown`, `diff`. Text and markdown go through the
transcript's own renderers. A unified diff (what `git diff` prints) is
converted to the reference row grammar the edit tool already uses — real
line numbers, `+`/`-` in the marker column, `⋯` between hunks — and painted
with the diff-marker tokens. There is no fourth format until a package needs
one.

### 4. UI — requests from the extension, answered by the user

This family reverses the direction of the protocol: the extension sends a
request with its own `id` and e replies. In `e rpc` (no terminal),
`initialize` says `"ui": false` and every UI request is answered with an
error immediately, so an extension can branch the way pi's `ctx.hasUI`
lets it.

```
ui.notify  {message, tone?}                          → {}
ui.show    {title?, body, format}                    → {}
ui.select  {title, options:[{label, description?, value}]} → {value} | {cancelled:true}
ui.confirm {title, message?}                         → {confirmed}
ui.input   {title, placeholder?, secret?}            → {text} | {cancelled:true}
ui.status  {text | null}                             → {}
ui.compose {text}                                    → {}
ui.panel   {title, lines, interactive?} | null       → {}
```

`select`, `confirm`, and `input` are modal and queue first-come across
extensions; Esc cancels the open one. They render through the existing
picker, trust-panel and composer surfaces, so they cannot diverge from the
look. `status` is one bounded slot per extension on the status row. `panel`
is a framed footer surface, one slot, last writer wins, Esc closes it.

An `interactive` panel is pi's custom component, done declaratively: while
it is open, e forwards keys as `ui.key {key}` notifications and the
extension re-sends the panel to redraw. Esc and ctrl+c never reach the
extension. Panel lines are plain strings, or arrays of `{text, token}`
spans painted with `theme.fg(token)`; unknown tokens paint plain; control
sequences are stripped. That is the whole styling surface. An extension
cannot emit an escape sequence, and its output is themed by the user's
palette like everything else.

### 5. Session — control, not custody

```
session.send      {content, internal?, run?}   → {}
session.info      {}                           → {path, id, name, cwd, model, effort, running, tools}
session.name      {name}                       → {}
session.model     {model}                      → {} | error
session.effort    {effort}                     → {} | error
session.tools     {names | null}               → {}
session.interrupt {}                           → {}
session.compact   {}                           → {}
```

`tools` is pi's `setActiveTools`: the names, built-in or extension, the
model may see and call until reset with `null`. It is how a plan mode
narrows the agent to read and grep without a second harness.

`send` is pi's `sendUserMessage` and `sendMessage` folded into one: a user
message, `internal` keeps it out of the transcript (it still reaches the
model), `run` starts a turn (the default only for non-internal messages).
Not offered: `newSession`, `fork`, `switchSession`, `navigateTree`. Those
replace the session under the user; the `/tree`, `/new`, and `/resume`
commands are the user's, and a command result can already submit a prompt.

### Shortcuts

The manifest may declare `shortcuts: [{key, description}]`. An unclaimed
chord (see `docs/keybindings.md` for what e keeps) becomes a `shortcut
{key}` request to the declaring extension, answered like a command. Two
extensions claiming one chord: first wins, with a notice.

### What stays out, and why

- **Providers.** pi's `registerProvider` adds code. e's providers are data
  (`providers/data/*.json`, `~/.e/models.json`), and the four dialects are
  the entire network surface. A new gateway is a registry entry.
- **Message renderers.** Rendering code in the extension is the thing the
  boundary exists to keep out. `format` covers the cases that matter.
- **Resource discovery at runtime.** Packages carry skills, prompts and
  themes as files; there is nothing for an extension to discover.
- **Persisted custom entries** (pi's `appendEntry`). They would add a
  message kind to the session format for state that belongs to the
  extension. `session.info` gives the session id and path; an extension
  keeps its own state keyed by them, and it survives resume without e
  learning a new record type.
- **Session replacement** (`newSession`, `fork`, `switchSession`,
  `navigateTree`). See section 5.

## Safety

- Every e→extension hook keeps its five-second budget and fails open.
- Every extension→e request is bounded: at most 32 in flight per
  extension, each line under 1 MiB, `status` clipped to 40 columns, `panel`
  to 200 lines, `show` bodies to 64 KiB. Past a bound, the request is
  answered with an error, never dropped silently.
- All extension text is sanitized before paint (`sanitize_display`): no
  control characters, no escape sequences. Tokens are theme names, so an
  extension can only use colours the user's theme defines.
- Modal UI belongs to one extension at a time. A reload, a session switch,
  or shutdown answers every pending request with an error rather than
  leaving a process waiting forever.
- `before_turn` appends; nothing an extension does removes the user's
  AGENTS.md, skills catalog, or system prompt from the request.
- `session.model` changes the model for the next request only through the
  same path `/model` uses; it cannot name a model the user has not signed
  in to.

## Consequences

- The protocol number stays 1. Every family is additive, manifest-declared,
  and capability-advertised: `capabilities: ["tool.update", "events",
  "hooks", "display", "ui", "session", "shortcuts"]`.
- `docs/extensions/scaffold.mjs` grows `ui.*` and `session.*` helpers that
  return promises, so an extension reads like pi's SDK.
- The `diff` package is the first consumer: `/diff` shows a real diff, the
  tool's result carries a diff `display`, and the per-turn line is a
  notice. Its previous plain-text output was the demonstration that the
  surface was missing.
- The interactive panel is the one place the design admits round trips per
  keystroke. If a package needs a richer component model than "lines and
  keys", that is a new decision, not a quiet extension of this one.
