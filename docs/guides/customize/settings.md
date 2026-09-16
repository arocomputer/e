---
title: Settings
description: settings.json, the home directory, and every preference
order: 2
---

# Settings

`~/.e/settings.json` holds your preferences, and `/settings` edits the same
file from inside a session. `/reload` applies a hand edit without restarting e.

## The home directory

Everything e remembers lives under `~/.e`:

| Path | What it holds |
| --- | --- |
| `settings.json` | the preferences below |
| `auth.json` | provider credentials stored by `/login` ([models](models.md)) |
| `models.json` | models you added or corrected ([models](models.md)) |
| `models-dev.json` | cached model facts read from models.dev |
| `sessions/` | one JSONL file per conversation ([sessions](../usage/sessions.md)) |
| `history.jsonl` | the prompts Up recalls on an empty composer |
| `extensions/` | programs e starts at launch ([extensions](../extend/extensions.md)) |
| `skills/` | `SKILL.md` folders ([skills](skills.md)) |
| `prompts/` | `/name` templates ([prompt templates](prompt-templates.md)) |
| `themes/` | palettes ([themes](themes.md)) |
| `keybindings.json` | composer chords ([keybindings](keybindings.md)) |
| `layout.json` | side panes and status rows ([layout](layout.md)) |
| `packages/` | package clones ([packages](../extend/packages.md)) |
| `AGENTS.md` | instructions that apply in every workspace |
| `trust.json` | which directories you trusted |

`E_HOME` moves the whole directory. That is how a preview build and a local
`./x dev` build stay out of the way: stable e uses `~/.e`, beta `~/.e-beta`,
and dev `~/.e-dev` ([install](../start/install.md)).

## Settings

| Key | Values | Default | What it changes |
| --- | --- | --- | --- |
| `theme` | `auto`, or a theme name | `auto` | the palette; `auto` follows the system |
| `tui_mode` | `inline`, `fullscreen` | `inline` | whether the conversation stays in the terminal's scrollback, or e pins the composer and scrolls it itself |
| `show_thinking` | `on`, `off` | `off` | whether the model's reasoning appears in the transcript |
| `auto_update` | `on`, `off` | `on` | the launch-time update check |
| `effort` | the model's levels | `high` | the reasoning effort a session starts at |
| `editor` | a command | `$VISUAL`, `$EDITOR`, then `vi` | where ctrl+g opens the draft |
| `tool_label_rows` | 1 to 20 | `2` | the height of a tool command's label |
| `tool_preview_rows` | up to 20 | `5` | the live output lines a running tool shows |
| `paste_placeholder` | codepoints | `1000` | when a paste collapses to a token; `0` keeps it raw |
| `paste_label` | a template | `[Pasted text #{id}, {chars} chars]` | the collapsed paste token |

A missing or invalid value keeps the default. `/settings` writes the same keys,
so a preference set there and one typed here mean the same thing.

## Keys e maintains

Two keys are records rather than preferences, and a command owns each:

- `packages` lists the package sources you installed and where each came from;
  `e install` and `e remove` write it ([packages](../extend/packages.md)).
- `scoped_models` lists the models ctrl+p cycles; `/scoped-models` writes it.

Change those through the command that owns them.
