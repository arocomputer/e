---
title: Keybindings
description: Rebind the composer's editing keys in keybindings.json.
order: 7
---

# Keybindings

`~/.e/keybindings.json` rebinds the composer's line-editing keys. This guide
also covers the keys that are not rebindable: the transcript reader, pasted
text, the external editor, and cancellation.

## Rebind a key

Map a chord to an action name:

```json
{
  "ctrl+j": "none",
  "alt+d": "kill_word"
}
```

Run `/reload` to apply the file; changing anything in `/settings` also
rereads it. Chords you leave out keep their default. A missing or malformed
file leaves every default in place, and an entry with an unknown action is
ignored.

`"none"` unbinds a chord: e swallows the key instead of typing it.

## Default bindings

| Chord | Action |
| --- | --- |
| `enter` | `enter` (submit) |
| `shift+enter`, `alt+enter`, `ctrl+j` | `newline` |
| `backspace` | `backspace` |
| `alt+backspace`, `ctrl+w` | `kill_word` |
| `delete`, `ctrl+d` | `delete` |
| `left`, `ctrl+b` | `left` |
| `right`, `ctrl+f` | `right` |
| `alt+left` | `word_left` |
| `alt+right` | `word_right` |
| `up`, `down` | `up`, `down` |
| `home`, `ctrl+a` | `home` |
| `end`, `ctrl+e` | `end` |
| `ctrl+k` | `kill_to_end` |
| `ctrl+u` | `kill_to_start` |

Shift with an arrow, `home`, or `end` extends a selection; typing replaces
the selection. Selection keys are not rebindable.

## Actions

| Action | Effect |
| --- | --- |
| `enter` | Submit the draft. |
| `newline` | Insert a line break. |
| `backspace` | Delete the selection or the character before the cursor. |
| `delete` | Delete the selection or the character after the cursor. |
| `left`, `right` | Move one character. |
| `up`, `down` | Move one line; on the first or last line, walk prompt history. |
| `word_left`, `word_right` | Move one word. |
| `home`, `end` | Jump to the start or end of the draft. |
| `kill_to_end` | Delete from the cursor to the end of the draft. |
| `kill_to_start` | Delete from the start of the draft to the cursor. |
| `kill_word` | Delete the selection or the word before the cursor. |

## Chords

A chord is `[ctrl+][alt+][shift+]<key>`. Modifiers may come in any order,
and the chord is case-insensitive. `control` is accepted for `ctrl`, and
`option` or `meta` for `alt`. `-` works as a separator too (`ctrl-w`).

`<key>` is `enter`, `backspace`, `delete`, `left`, `right`, `up`, `down`,
`home`, `end`, or a single character. The character may be `+` or `-`
itself, as in `ctrl+-` and `ctrl++`: e reads modifiers off the front and the
rest is the key.

Write a capital letter with its modifier, `shift+a`, because that is how the
terminal reports it.

The same grammar names the pane focus chord in [layout](layout.md) and
extension shortcuts.

## Keys the keymap never sees

e handles these before the keymap, so binding them here has no effect:

- `ctrl+c`, `ctrl+o`, `ctrl+g`, `ctrl+p` and `ctrl+shift+p`, `shift+tab`
- `ctrl+v` and Command+V (clipboard paste)
- `esc` while a turn runs
- PageUp, PageDown, and End while chat is scrolled
- Up, Down, Enter, Tab, and Esc while a picker is open
- the layout's focus chord (`ctrl+t` by default) while a side pane is open

An extension's declared shortcut fires only when neither e nor this keymap
used the chord. Unbind a composer chord with `"none"` to hand it to an
extension. See [extensions](../extend/extensions.md#shortcuts).

## External editor

Ctrl+G opens the draft in an external editor: the `editor` setting, else
`$VISUAL`, else `$EDITOR`, else `vi`. Save and quit to bring the text back. A
non-zero exit leaves the draft unchanged.

## Prompt history

Up on an empty composer recalls earlier prompts, across sessions. e keeps the
newest thousand in `~/.e/history.jsonl`, readable only by you, and skips a
prompt identical to the one before it. See
[sessions](../usage/sessions.md).

## Transcript reader

Ctrl+O opens the transcript reader at the latest output. It shows the
thinking and tool calls that normal chat folds away. It uses the alternate
screen, so expanded output does not enter your scrollback, and closing it
restores the previous view and the draft.

The reader has two depths:

- **Review** folds each tool detail to three lines behind a `→ to expand`
  hint.
- **Full** shows every row.

| Key | Effect |
| --- | --- |
| Up, Down | Scroll one row. |
| Mouse wheel | Scroll `scroll_lines` rows (default 3). |
| PageUp, PageDown | Scroll a page. |
| Home, End | Jump to the top or bottom. |
| Left, Right | Switch to Review or Full. |
| Ctrl+O, Esc | Close the reader. |
| Ctrl+C | Close the reader and cancel, as everywhere else. |

Scrolling up pauses following new output; returning to the bottom resumes
it. Typing and pasting in the reader leave the draft alone.

The footer reads `Review · ←/→ switch · ctrl o close · PgUp/PgDn scroll ·
Esc close`, or `Full detail · …` at the Full depth. The `transcript_hint`
[setting](settings.md#wording) replaces it at both depths the next time the
reader opens.

## Pasted text

A paste longer than `paste_placeholder` codepoints (default 1000) collapses
into a marker such as `[Pasted text #1, 42000 chars]`, drawn in the theme's
`dim` tone. Submitting expands each marker to its text once. Set the
threshold and the label with `paste_placeholder` and `paste_label` in
[settings](settings.md#conversation-display).

- The count is Unicode codepoints, after CRLF and lone CR become newlines.
- Numbers identify markers in the current draft. A new paste takes the next
  number after the highest remaining one, and numbering restarts at `#1`
  when no markers remain or the draft is submitted or cleared.
- Deleting or replacing any part of a marker removes the whole marker and
  its text. Retyping the marker does not bring the text back.
- Clearing or replacing the draft discards its markers. Walking prompt
  history keeps the unsent draft and its markers until you return to it.
- An empty `paste_label` inserts the full text instead of a marker.

## Draft display

Terminal control characters in a draft display as replacement characters,
and tabs display as spaces; the draft submits them unchanged. Up and Down
keep the display column across wide and combining characters.

A draft that starts with `!` runs as a shell command. The composer shows it
by drawing the first gutter as a green `!` in the theme's `bashMode` tone.
When the prefix is `! `, its space stays editable in the gutter. Deleting the
`!` restores the normal composer.

## Cancel and quit

Ctrl+C works everywhere. The first press:

- cancels the running turn, any sign-in, and any open extension prompt
- closes every open panel, picker, side pane, and the transcript reader
- clears the draft and any held launch prompt
- arms exit

Press it again within 1.5 seconds to quit.

## Trust question keys

Up and Down change the choice; Enter confirms. When the question is taller
than the terminal, PageUp and PageDown scroll it without changing the choice.
Quitting at the question records nothing, and neither does declining. See
[instructions](instructions.md#trust).
