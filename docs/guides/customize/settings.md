---
title: Settings
description: Your preferences, and every file e keeps under ~/.e.
order: 2
---

# Settings

This guide lists every key in `~/.e/settings.json` and every file e keeps in
its home directory. Read it when you want to change a preference or find
where e stored something.

## Change a setting

Run `/settings` to change the theme, TUI mode, thinking display,
auto-update, and reasoning effort from inside a session. Every other key is
set by editing the file:

```json
{
  "theme": "dark",
  "tui_mode": "fullscreen",
  "editor": "code --wait",
  "scroll_lines": 5
}
```

After a hand edit, run `/reload` to apply it without restarting. A missing or
invalid value keeps the default. e merges its own writes into the file, so
keys it does not know survive.

## Interface

These five keys are the ones `/settings` shows.

| Key | Values | Default | What it changes |
| --- | --- | --- | --- |
| `theme` | `auto`, `dark`, `light`, or a [theme](themes.md) name | `auto` | The palette. `auto` follows the terminal's background and uses `dark` when it cannot tell. |
| `tui_mode` | `inline`, `fullscreen` | `inline` | `inline` keeps the conversation in the terminal's scrollback. `fullscreen` uses the alternate screen, pins the composer, and scrolls the conversation itself. |
| `show_thinking` | `on`, `off` | `off` | `off` folds reasoning behind a one-line hint; `on` expands it. Ctrl+O shows folded thinking either way. |
| `auto_update` | `on`, `off` | `on` | The launch-time background update. It applies only to release builds; a build from source never updates itself. |
| `effort` | the current model's levels | `high` | The reasoning effort a session starts at. Shift+Tab changes it and saves the new level here. |

## Conversation display

| Key | Values | Default | What it changes |
| --- | --- | --- | --- |
| `scroll_lines` | 1 to 100 | `3` | Rows per mouse-wheel step in chat and in the transcript reader. |
| `tool_label_rows` | 1 to 20 | `2` | Rows a tool call's command label may take. |
| `tool_preview_rows` | 0 to 20 | `5` | Live output lines a running tool shows. |
| `tool_history_limit` | 0 to 1000 | `10` | Recent successful tool calls shown per group. Failures and running tools stay visible, and Ctrl+O shows every call. |
| `editor` | a command | `$VISUAL`, then `$EDITOR`, then `vi` | The editor Ctrl+G opens the draft in. e splits the value on whitespace and appends the draft's path. |
| `paste_placeholder` | codepoints | `1000` | A paste longer than this collapses into a marker. `0` keeps pastes raw. Read at launch. |
| `paste_label` | a template | `[Pasted text #{id}, {chars} chars]` | The collapsed paste marker. Fields: `{id}`, `{chars}`, `{lines}`, `{plural}` (empty for one line, `s` otherwise). Read at launch. |

The wheel and PageUp/PageDown scroll chat in both TUI modes without touching
the draft. Scrolling up pauses following new output; End, scrolling back to
the bottom, or submitting a prompt resumes it. Inline mode borrows the
alternate screen while you read earlier rows. To select text with the mouse
while e captures mouse events, hold your terminal's selection modifier,
usually Shift.

## Wording

Each of these replaces a line of e's own text.

| Key | Default | Where it appears |
| --- | --- | --- |
| `scroll_hint` | `Scrolled · End to follow` | The status row while you read earlier chat. |
| `thinking_hint` | `Thinking · ctrl o to view` | The row that stands for folded reasoning. |
| `tool_history_hint` | `{count} earlier successful tools · ctrl o to view` | The summary of folded successful tool calls. |
| `transcript_hint` | per depth, see [keybindings](keybindings.md#transcript-reader) | The transcript reader's footer, at both depths. |
| `trust_scroll_hint` | `↑↓ Choose · Enter Continue · PgUp/PgDn Scroll` | The trust question when it is taller than the terminal. |
| `no_answer_message` | `The model finished without an answer. Retry or ask it to continue.` | The warning after a turn that produced reasoning but no answer. |

## Agent

| Key | Values | Default | What it changes |
| --- | --- | --- | --- |
| `model` | `provider/id` | the first signed-in model | The model a session starts with. `/models` writes it. |
| `system_prompt` | text | e's built-in prompt | Replaces the base system prompt. [Instructions](instructions.md), the skills catalog, the working directory, platform, and date are still appended. |
| `no_tools_notice` | text | `This run has no tools. Answer without attempting tool calls.` | Appended to the system prompt when a run has no tools. |
| `tool_allowlist_notice` | text with `{tools}` | `This run may use only these tools: {tools}. Do not attempt other tool calls.` | Appended when a run allows only some tools. `{tools}` is the comma-separated list. |
| `tool_concurrency` | 1 to 64 | `8` | How many tool calls from one reply run at once. Calls that name the same file run in the order the model gave them. |
| `retry_max_attempts` | 0 to 20 | `10` | Provider requests one failure may use, the first included, before the turn fails. Quota errors are never retried. |
| `sleep_window_secs` | seconds | `300` | A turn interrupted by a system sleep at least this long stops instead of resuming. |
| `sleep_continuations` | a count | `3` | How many times one turn may resume a reply cut off by system sleep. |

## Keys e maintains

A command owns each of these keys. Change them through that command rather
than by hand.

| Key | Written by | What it holds |
| --- | --- | --- |
| `scoped_models` | `/scoped-models` | The `provider/id` list Ctrl+P cycles. Absent means every signed-in model. |
| `packages` | `e install`, `e remove` | Installed package sources. See [packages](../extend/packages.md). |
| `format_version` | e | The file's format. |

`extensions` is yours to write: an object with one entry per extension,
keyed by its name, that e passes to that extension at launch. See
[extensions](../extend/extensions.md).

## The home directory

Everything e remembers lives in one home directory. This guide and the
others call it `~/.e`, the home of release builds. The actual directory
depends on how e was built:

| Build | Home |
| --- | --- |
| Built from source (`cargo install`, `cargo build`, `./x dev`), which is every build today | `~/.e-dev` |
| PR preview | `~/.e-pr/<commit>` |
| Release | `~/.e` |

`E_HOME=/path` overrides all three. See [install](../start/install.md).

| Path | What it holds |
| --- | --- |
| `settings.json` | The preferences above. |
| `auth.json` | Provider credentials stored by `/login`. See [models](models.md#credentials). |
| `models.json` | Models you added or corrected. See [models](models.md). |
| `models-store.json` | Cached model lists from each signed-in provider. |
| `models-dev.json` | Cached model facts from models.dev. |
| `sessions/` | One JSONL file per session. See [sessions](../usage/sessions.md). |
| `history.jsonl` | Prompts that Up recalls on an empty composer. |
| `trust.json` | Your trust decisions per directory. See [instructions](instructions.md#trust). |
| `AGENTS.md` | Instructions for every workspace. See [instructions](instructions.md). |
| `skills/` | `SKILL.md` folders. See [skills](skills.md). |
| `prompts/` | `/name` templates. See [prompt templates](prompt-templates.md). |
| `themes/` | Palettes. See [themes](themes.md). |
| `keybindings.json` | Composer key overrides. See [keybindings](keybindings.md). |
| `layout.json` | Side panes and status rows. See [layout](layout.md). |
| `extensions/` | Programs e starts at launch. See [extensions](../extend/extensions.md). |
| `packages/` | Installed packages. See [packages](../extend/packages.md). |

A trusted workspace can carry its own `skills/`, `prompts/`, and `packages`
list in a `.e/` directory in the workspace.
