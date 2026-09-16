---
title: Layout
description: Place side panes and shape the status rows in layout.json.
order: 8
---

# Layout

`~/.e/layout.json` sets where the regions of e's frame go and what the status
row reads. It follows the same file-backed pattern as themes and keybindings.

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

Every key is optional. The values above are the defaults, except the `diff`
entry, which is an example.

Apply changes with `/reload`, or after you close `/settings`. A missing or
malformed file falls back to e's built-in layout untouched.

## Panes

An extension opens a side pane with `ui.pane` and may propose a side. See
`docs/guides/extend/extensions.md`. Your `panes` entries outrank the
extension's proposal.

An entry named after the pane's id sets two things:

- `side` is `left` or `right`.
- `width` is the pane's share of the terminal, 30 to 70 percent.

The `*` entry is the default for every pane you do not name.

Below `split_min` columns, the pane and the conversation cannot sit side by
side. The focused one fills the screen, and the status row says how to reach
the other.

`focus` is the chord that moves focus between the conversation and the pane.
Any ctrl or alt chord that e does not already use works. The grammar is the
one keybindings use. See `docs/guides/customize/keybindings.md`.

## The status row

`status.left` and `status.right` are lists of segments. The `{tokens}` in each
segment expand.

| Token | Value |
| --- | --- |
| `{model}` | the current model, compact |
| `{effort}` | its selected reasoning effort |
| `{context}` | the context in use, as a percent, blank under 1% |
| `{cwd}` | the working directory, shortened like the tab title |
| `{session}` | the session's name, once it has one |
| `{status}` | every extension's `ui.status` slot, joined with ` · ` |
| `{status:<name>}` | one extension's slots |

e drops a segment whose tokens all came up empty. It also drops a ` / ` part
around an empty token. So `"{model} / {effort}"` reads just the model for a
model without an effort knob.

The first left segment paints accent-bright. The rest paint muted, after
` · `. The right side sits right-aligned. It gives way to a transient notice,
such as the armed-exit hint, a clipboard read, or a hidden pane.

## The activity row

`activity` is the row below the transcript while a turn runs. It is one
template. By default it reads `Thinking (3s) (↑1k ↓20)`.

| Token | Value |
| --- | --- |
| `{phase}` | `Thinking`, `Compacting context`, the retry line, or the recovered flash |
| `{elapsed}` | the turn's clock, `(3s)` |
| `{tokens}` | the turn's token flow, `(↑1k ↓20)` |
| `{activity}` | what extensions put there with `ui.activity`, joined with ` · ` |

`"activity": "{phase} {elapsed}"` drops the token counts. `"{phase}"` leaves
just the word.

An empty token leaves no gap. A ` · ` or ` / ` part that came up empty goes
with its separator. Between turns, the row shows extension text alone, when
there is any.

## The banner

`"banner": false` leaves out the `𝑒 <version> · Run /help for commands` line
at the top of a session.
