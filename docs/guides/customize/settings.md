---
title: Settings
description: Your preferences, and every file e keeps under ~/.e.
order: 2
---

# Settings

`~/.e/settings.json` holds your preferences. `/settings` edits the same file
from inside a session. After you edit the file by hand, run `/reload` to apply
it without restarting e.

## Settings

| Key | Values | Default | What it changes |
| --- | --- | --- | --- |
| `theme` | `auto`, or a theme name | `auto` | The palette. `auto` follows the system. |
| `tui_mode` | `inline`, `fullscreen` | `inline` | `inline` keeps the conversation in the terminal's scrollback. `fullscreen` pins the composer and e scrolls the conversation itself. |
| `show_thinking` | `on`, `off` | `off` | Expand reasoning, or retain it behind a one-line hint. Ctrl+O reveals thinking received during this session. |
| `auto_update` | `on`, `off` | `on` | The launch-time update check. |
| `effort` | the model's levels | `high` | The reasoning effort a session starts at. |
| `editor` | a command | `$VISUAL`, `$EDITOR`, then `vi` | Where ctrl+g opens the draft. |
| `tool_label_rows` | 1 to 20 | `2` | The height of a tool command's label. |
| `tool_preview_rows` | up to 20 | `5` | How many live output lines a running tool shows. |
| `tool_history_limit` | 0 to 1000 | `10` | Recent successful tools shown per group. Failures and running tools stay visible, and Ctrl+O shows every call. |
| `scroll_lines` | 1 to 100 | `3` | Rows per wheel event in chat and the detail reader. |
| `scroll_hint` | text | `Scrolled · End to follow` | Status text while reading earlier chat. |
| `thinking_hint` | text | `Thinking · ctrl o to view` | The collapsed reasoning row. |
| `tool_history_hint` | a template | `{count} earlier successful tools · ctrl o to view` | Summary of folded successful tools. |
| `no_answer_message` | text | `The model finished without an answer. Retry or ask it to continue.` | Warning when a completed turn contains reasoning but no answer. |
| `paste_placeholder` | codepoints | `1000` | The size at which a paste collapses to a token. `0` keeps pastes raw. |
| `paste_label` | a template | `[Pasted text #{id}, {chars} chars]` | The collapsed paste token. |

A missing or invalid value keeps the default. `/settings` writes the same
keys, so a preference set there means the same as one typed in the file.

The wheel and PageUp/PageDown scroll chat in both TUI modes without changing
the draft. Scrolling upward pauses following output. End, scrolling to the
bottom, or submitting a prompt resumes following. Inline mode keeps native
terminal history at the tail and temporarily uses the alternate screen while
reading earlier rows. Fullscreen uses the alternate screen throughout.
Hold your terminal's selection modifier, commonly Shift, for native mouse
selection while e captures mouse events.

Existing `show_thinking: "off"` settings now collapse reasoning instead of
discarding it from the live display. Changing it to `on` reveals thinking
already received in the current session. Settings files need no migration.

## Keys e maintains

Two keys are records, not preferences. A command owns each one, so change them
through that command:

- `packages` lists the package sources you installed and where each came from.
  `e install` and `e remove` write it. See [packages](../extend/packages.md).
- `scoped_models` lists the models ctrl+p cycles. `/scoped-models` writes it.

## The home directory

Everything e remembers lives under `~/.e`:

| Path | What it holds |
| --- | --- |
| `settings.json` | The preferences above. |
| `auth.json` | Provider credentials stored by `/login`. See [models](models.md). |
| `models.json` | Models you added or corrected. See [models](models.md). |
| `models-dev.json` | Cached model facts read from models.dev. |
| `sessions/` | One JSONL file per conversation. See [sessions](../usage/sessions.md). |
| `history.jsonl` | The prompts Up recalls on an empty composer. |
| `extensions/` | Programs e starts at launch. See [extensions](../extend/extensions.md). |
| `skills/` | `SKILL.md` folders. See [skills](skills.md). |
| `prompts/` | `/name` templates. See [prompt templates](prompt-templates.md). |
| `themes/` | Palettes. See [themes](themes.md). |
| `keybindings.json` | Composer chords. See [keybindings](keybindings.md). |
| `layout.json` | Side panes and status rows. See [layout](layout.md). |
| `packages/` | Package clones. See [packages](../extend/packages.md). |
| `AGENTS.md` | Instructions that apply in every workspace. |
| `trust.json` | Which directories you trusted. |

### Move the home with `E_HOME`

`E_HOME` moves the whole directory. This keeps a preview build and a local
`./x dev` build out of each other's way. Stable e uses `~/.e`, beta uses
`~/.e-beta`, and dev uses `~/.e-dev`. See [install](../start/install.md).
