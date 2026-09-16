---
title: Keybindings
description: Rebind the composer's editing keys in keybindings.json.
order: 7
---

# Keybindings

`~/.e/keybindings.json` overrides the composer's line-editing keys. It follows
the same file-backed pattern as themes and skills.

```json
{
  "ctrl+j": "none",
  "alt+d": "kill_word"
}
```

Each key is a chord and each value is an action. Apply changes instantly with
`/reload`, or after you close `/settings`. A missing or malformed file falls back
to e's built-in bindings untouched.

## Chords

A chord is `[ctrl+][alt+][shift+]<key>`. Modifiers may come in any order, and
chords are case-insensitive.

`<key>` is one of `enter`, `backspace`, `delete`, `left`, `right`, `up`,
`down`, `home`, `end`, or a single character. The character may be `+` or
`-`, as in `ctrl+-` and `ctrl++`. e reads modifiers off the front, and
whatever remains is the key.

Spell a capital letter with its modifier, `shift+a`, because that is how the
terminal reports it.

## Actions

The value is one of these action names:

- `enter`, `newline`
- `backspace`, `delete`
- `left`, `right`, `up`, `down`
- `word_left`, `word_right`
- `home`, `end`
- `kill_to_end`, `kill_to_start`, `kill_word`

Use `"none"` to unbind a built-in chord. e swallows the key instead of typing
it as a literal character.

## Which chords reach the keymap

e's application-level shortcuts run first. Only chords they do not claim
reach this keymap, so binding one of these here has no effect:

- ctrl+c
- ctrl+o
- ctrl+p
- ctrl+v or Command+V, for clipboard image or text paste
- tab
- shift+tab
- menu navigation

An extension's declared shortcut runs after this keymap. See Shortcuts in
`docs/guides/extend/extensions.md`. A chord bound here, or by the composer's
built-in bindings, never reaches the extension. Unbind it here with `"none"`
to hand it over. Extensions may only declare ctrl or alt chords.

The chord that moves focus between the conversation and a side pane is not
set here. It is `ctrl+t` by default, and you set it in `~/.e/layout.json`. See
`docs/guides/customize/layout.md`.

## External editor

ctrl+g opens the draft in an external editor. e uses the first of these that
is set:

1. the `editor` setting in `~/.e/settings.json`, such as
   `"editor": "code --wait"`
2. `$VISUAL`
3. `$EDITOR`
4. `vi`

Save and quit to bring the text back. A non-zero exit leaves the draft
unchanged.

## Prompt history

↑ on an empty composer recalls earlier prompts, including prompts from
previous sessions. e keeps the newest thousand in `~/.e/history.jsonl`,
private to your user. A prompt identical to the last one is not repeated.

## Full transcript

`Ctrl+O` opens the review screen at the latest output. Tool details wrap with
the reference's `│` rails, and the rail connector renders in the theme's
`muted` tone. The footer has a navigation row, a blank row, and the usual
model and context status.

The screen has two depths:

- Review folds each tool detail to three lines behind a `→ to expand` hint.
- Full shows every row.

The reader's keys:

- `Up`/`Down` scroll one row. The mouse wheel scrolls three.
- `PageUp`/`PageDown` scroll a page. `Home`/`End` jump to the ends.
- `←`/`→` switch between the Review and Full depths.
- Scrolling up pauses following new output. Returning to the bottom resumes it.
- `Ctrl+O` or `Esc` closes the reader. `Ctrl+C` closes it and keeps e's global
  cancellation behavior.
- Typing and pasting in the reader leave the draft alone.

The reader uses the alternate terminal screen, so expanded tool output does
not replace your normal scrollback. Closing the reader returns to the previous
view and preserves the draft.

### Footer wording

Set `transcript_hint` in `~/.e/settings.json` to override the footer wording.
It applies the next time you open the reader, at both depths. The defaults
are:

- `Review · ←/→ switch · ctrl o close · PgUp/PgDn scroll · Esc close`
- `Full detail · ←/→ switch · ctrl o close · PgUp/PgDn scroll · Esc close`

## Pasted text

A long paste collapses into a marker such as `[Pasted text #1, 42000 chars]`.
The marker uses the same `dim` grey as image attachments.

### Counting

The character count is Unicode codepoints, after CRLF and standalone CR
normalize to newlines. The label does not count source lines or wrapped
screen rows.

Numbers identify collapsed pastes in the current draft, not every clipboard
operation in the session. Existing numbers stay stable. A new paste takes the
next number after the highest remaining one. Numbering restarts at `#1` when
no markers remain, or when you submit or clear the draft.

### Editing markers

- Deleting or replacing any part of a marker removes the whole marker and its
  stored text.
- Retyping a deleted marker cannot bring the text back.
- Clearing or replacing the draft also discards its attachments.
- History navigation preserves the unsent draft and its attachments until you
  return, submit, or clear it.
- Submission expands each surviving attachment once.

### Paste settings

These preferences in `~/.e/settings.json` take effect in a new editor:

- `paste_placeholder` collapses pastes above this codepoint count. The default
  is `1000`, and `0` disables collapsing.
- `paste_label` sets the marker text. The default is
  `"[Pasted text #{id}, {chars} chars]"`. The optional `{lines}` and
  `{plural}` fields count source lines. `{plural}` is empty for one line and
  `s` otherwise. An empty label inserts the full text rather than creating an
  invisible attachment.

## Draft display

Pastes normalize CRLF and standalone CR to one newline each. Terminal control
characters in a draft display as replacement characters, and tabs display as
spaces. The underlying draft keeps those characters for submission. Up/Down
preserve display columns across wide and combining characters.

## Global cancellation and trust navigation

Ctrl+C works in every panel. The first press does all of this:

- cancels active work and sign-in
- clears the draft and any held launch prompt
- closes trust and queue navigation
- arms exit

Press it again within 1.5 seconds to quit.

Quitting at the trust question does not record a trust decision. Neither does
the question's last row: declining exits, and the next launch asks again.

Long trust questions and choices wrap. If they exceed the terminal height,
PgUp/PgDn scroll the text without changing the choice. Up/Down change the
choice and reveal its label. Override the scrolling hint with
`"trust_scroll_hint"` in `~/.e/settings.json`.

## Shell composer

Typing `!` as the first character replaces the first `┃` gutter with a green
`!`, using the theme's `bashMode` token. Command text keeps its normal color,
and wrapped lines keep neutral rails. Deleting the leading `!` restores the
normal composer.

The draft and the submitted command keep the original prefix. When that
prefix is `! `, its space stays editable in the gutter, with its own cursor
and selection highlight.
