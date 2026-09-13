# Keybindings

`~/.e/keybindings.json` overrides the composer's line-editing keys — the
same file-backed pattern as themes and skills. A missing or malformed file
falls back to e's built-in bindings untouched.

```json
{
  "ctrl+j": "none",
  "alt+d": "kill_word"
}
```

- A chord is `[ctrl+][alt+][shift+]<key>`, any order, case-insensitive.
  `<key>` is `enter`, `backspace`, `delete`, `left`, `right`, `up`, `down`,
  `home`, `end`, or a single character.
- The value is an action name — `enter`, `newline`, `backspace`, `delete`,
  `left`, `right`, `up`, `down`, `word_left`, `word_right`, `home`, `end`,
  `kill_to_end`, `kill_to_start`, `kill_word` — or `"none"` to unbind a
  built-in chord (the key is swallowed, not typed as a literal character).
- Only chords not already claimed by e's application-level shortcuts
  (ctrl+c, ctrl+o, ctrl+p, tab, shift+tab, menu navigation) reach this
  keymap — binding one of those here has no effect, since the app-level
  handler runs first.

Apply instantly with `/reload` (or after closing `/settings`).

## Full transcript

`Ctrl+O` opens full detail at the latest output. Tool results wrap with a
primary-colour `│` and two spaces before the dim text, matching fx's current
reader. The footer has a navigation row, a blank row, and the usual model and
context status. There is no separate Review depth.

- `Up`/`Down` scroll one row; the mouse wheel scrolls three.
- `PageUp`/`PageDown` scroll a page. `Home`/`End` jump to the ends.
- Scrolling up pauses following new output. Returning to the bottom resumes it.
- `Ctrl+O` or `Esc` closes the reader. `Ctrl+C` closes it and retains e's global
  cancellation behavior. Typing and pasting in the reader leave the draft alone.

The reader uses the alternate terminal screen, so it does not replace normal
scrollback with expanded tool output. Opening it from `/diff` keeps the diff
panel underneath. Closing returns to the previous view and preserves the draft.

Set `transcript_hint` in `~/.e/settings.json` to override the footer wording.
It applies on the next open. The default is
`full detail · ctrl+o close · pgup/pgdn scroll · esc close`.
Themes can override `toolDetailRail`, which defaults to the terminal foreground.

The reference layout is in
[fx's transcript footer](https://github.com/vercel-labs/fx/blob/8f2271f89466133b9ad3c591b5a6d5199444c7e7/src/ui/footer/paint_plan.zig).
e implements the layout in Rust, without importing fx's implementation.

## Pasted text

Long pastes collapse into a marker such as
`[Pasted text #1, 42000 chars]`, in the same `dim` grey as image attachments.
Characters count Unicode codepoints after CRLF and standalone CR normalize
to newlines. The label does not count source lines or wrapped screen rows.

Numbers identify collapsed pastes in the current draft, not every clipboard
operation in the session. Existing numbers stay stable. New pastes use the
next number after the highest remaining one, restarting at `#1` when none
remain or when you submit or clear the draft.

Deleting or replacing any part of a marker removes the whole marker and its
stored text. Clearing or replacing the draft also discards its attachments.
Retyping a deleted marker cannot bring the text back. History navigation
preserves the unsent draft and its attachments until you return, submit, or
clear it. Submission expands each surviving attachment once.

These preferences in `~/.e/settings.json` take effect in a new editor:

- `paste_placeholder`: collapse pastes above this codepoint count. Default
  `1000`; `0` disables collapsing.
- `paste_label`: default
  `"[Pasted text #{id}, {chars} chars]"`.
  Optional `{lines}` and `{plural}` fields count source lines. `{plural}` is
  empty for one line and `s` otherwise. An empty label inserts
  the full text rather than creating an invisible attachment.

## Diff review

`/diff` opens a mouse-driven review document above the shared composer.
Scroll the pane with the wheel, click file summaries to jump, and drag source
to add an inline diff attachment on release. Keyboard input stays with the
composer. Enter sends the prompt without closing review. Backspace selects a
diff attachment first; a second Backspace removes its payload and marker.

See `e docs diff` for comparison rules and preferences.
