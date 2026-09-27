---
title: Layout
description: Place side panes and shape the status rows in layout.json.
order: 8
---

# Layout

`~/.e/layout.json` decides where extension side panes go and what the status
row and activity row say. Read this guide to move a pane, change the focus
key, or rewrite the status line.

## Edit the layout

Every key is optional. This file spells out the defaults, plus one example
`diff` entry:

```json
{
  "split_min": 110,
  "focus": "ctrl+t",
  "banner": true,
  "panes": {
    "*":    { "side": "right", "width": 40 },
    "diff": { "side": "left",  "width": 50 }
  },
  "status": {
    "left":  ["{model} / {effort}", "{context}"],
    "right": ["{status}"]
  },
  "activity": "{phase} {elapsed} {tokens} · {activity}"
}
```

Run `/reload` to apply it; changing anything in `/settings` also rereads it.
A missing or malformed file leaves the built-in layout in place.

| Key | Default | What it sets |
| --- | --- | --- |
| `split_min` | `110` | Terminal columns needed to show a pane beside the conversation. Values below `60` count as `60`. |
| `focus` | `ctrl+t` | The chord that moves focus between the conversation and a side pane. |
| `banner` | `true` | `false` hides the `e <version> · Run /help for commands` line at the top of a session. |
| `panes` | `{}` | Side and width per pane id. See [panes](#panes). |
| `status.left`, `status.right` | see above | The status row's segments. See [the status row](#the-status-row). |
| `activity` | see above | The row shown while a turn runs. See [the activity row](#the-activity-row). |

## Panes

An extension opens a side pane with `ui.pane` and may propose a side. See
[extensions](../extend/extensions.md). A `panes` entry keyed by the pane's id
sets:

- `side`: `left` or `right`.
- `width`: the pane's share of the terminal width, in percent, clamped to 30
  to 70.

The `*` entry covers every pane you do not name. Each field resolves
separately:

| Field | First match wins |
| --- | --- |
| `side` | the pane's own entry, the extension's proposal, `*`, then `right` |
| `width` | the pane's own entry, `*`, then `40` |

Below `split_min` columns the pane and the conversation cannot sit side by
side. The focused one fills the screen. While the pane is hidden, the status
row names it and its focus chord.

`focus` uses the chord grammar from [keybindings](keybindings.md#chords).
Choose a ctrl or alt chord e does not already use; Ctrl+C, Ctrl+V, and Ctrl+O
never reach it.

## The status row

`status.left` and `status.right` are lists of segments. Each `{token}` in a
segment expands:

| Token | Value |
| --- | --- |
| `{model}` | the current model, compact |
| `{effort}` | its reasoning effort |
| `{context}` | the context in use, as a percent; blank under 1% |
| `{cwd}` | the working directory, shortened like the tab title |
| `{session}` | the session's name, once it has one |
| `{status}` | every extension's `ui.status` slot, joined with ` · ` |
| `{status:<name>}` | the slots of one extension |

A segment whose tokens are all empty is dropped. A ` / ` or ` · ` part that
expands to nothing is dropped with its separator, so `"{model} / {effort}"`
shows only the model when the model has no effort levels.

The first left segment is drawn in the `accent` tone and the rest in `muted`,
joined with ` · `. The right side is right-aligned. A transient notice takes
its place while it shows: the armed-exit hint, a clipboard read, or a hidden
pane.

## The activity row

`activity` is the row below the transcript while a turn runs. The default
renders as `Thinking (3s) (↑1k ↓20)`.

| Token | Value |
| --- | --- |
| `{phase}` | `Thinking`, `Compacting context`, the retry line, or the recovered notice |
| `{elapsed}` | the turn's clock, such as `(3s)` |
| `{tokens}` | the turn's token flow, such as `(↑1k ↓20)` |
| `{activity}` | text extensions set with `ui.activity`, joined with ` · ` |

`"activity": "{phase} {elapsed}"` drops the token counts, and `"{phase}"`
leaves only the word. An empty token leaves no gap, and an empty ` · ` or
` / ` part goes with its separator. Between turns the row shows extension
text alone, when there is any.
