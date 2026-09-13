# Diff review

Run `/diff` to open or close a live, read-only Git diff beside the conversation.
The conversation stays on the left, with a narrower diff pane on the right.
One full-width composer sits below both. The pane is a continuous document:
file counts at the top, then each file's header and source. Click a file in the
summary to jump to its changes, or scroll through the whole document.

Source rows show line numbers, syntax colors, and addition/removal backgrounds.
Changed words have stronger backgrounds. Removed rows use old line numbers;
context and added rows use new ones. Long lines wrap without repeating their
line number. Hunk headers are hidden, with separators between disjoint hunks.
Very large replacements use a bounded word comparison.

The comparison is the current workspace versus `HEAD`, including staged and
unstaged changes together and non-ignored untracked files. It includes changes
made outside e. A repository without commits compares its current files to an
empty tree. Renames appear as a deletion and an addition. This version does not
provide session-only or branch-base comparisons, staging, or reverting.

The panel refreshes after tool completion and periodically while visible.
An unchanged refresh preserves selection and scroll. If source changes during
a drag, that selection clears rather than attaching different text. Existing
draft attachments remain immutable snapshots.

## Interaction

The mouse controls review. Keyboard input stays with the composer, so Enter
submits your prompt without closing the diff. There is no diff focus mode,
Ctrl+D shortcut, or navigation hint on the status line.

- Wheel over the pane to scroll. Shift+wheel moves a page.
- Click a file summary to jump to that file's source.
- Drag across source rows. Releasing the mouse adds a blue `⧉ 4 lines from diff`
  marker to the composer. A single source row reads `⧉ 1 line from diff`.
- Selecting again replaces that marker and its payload in place, leaving your
  other draft text and caret position intact.
- The first Backspace at the marker selects it. A second removes it and its
  payload. Moving away cancels the selection. Ordinary pasted-text deletion
  keeps its existing behavior.
- Click the header's `×`, or run `/diff` again, to close review.

Attachments contain source text and file paths, without renderer styling, diff
signs, line numbers, or hunk headers. Tabs and indentation survive. Selecting
wrapped fragments attaches each logical source line only once. A selection can
cross file boundaries; the payload identifies each file. Selection never submits
by itself, and selecting a new range does not change an already submitted prompt.

The split needs 110 columns by default. On narrower terminals the diff fills
the area above the composer. Closing it restores chat and scrollback. Review
uses the alternate terminal screen. Resizing reflows wrapped source and clears
transient selection without changing an attachment already in the draft.

## Preferences

These keys in `~/.e/settings.json` apply when you next open the panel:

| Key | Default | Purpose |
| --- | --- | --- |
| `diff_min_width` | `110` | Minimum split width, at least 60 columns |
| `diff_width_percent` | `40` | Diff share of the terminal, between 30 and 70 percent |
| `diff_refresh_ms` | `1000` | Periodic refresh interval, at least 250 ms |
| `diff_title` | `{count} {files} changed` | Header, with file count and singular/plural noun |
| `diff_selection_label` | `⧉ {count} {lines} from diff` | Inline attachment label, with singular/plural line count |

Colors use the active theme. `border` colors dividers and `dim` colors
secondary text and attachment markers. The `diff*` tokens control the code
pane, selection, attachment marker, word highlights, syntax, line numbers, and counts. See
`e docs themes` for the token list. Other code blocks keep their existing palette.

Git reads have a five-second timeout and bounded output. Each refresh includes
at most 128 patches and 4 MiB of patch text, and stops starting patches after
five seconds. Omitted files are reported at the end of the document. Patches over 256 KiB
show a truncation notice. Binary files have no line counts. New files over
2 MiB or containing non-UTF-8/binary data show a preview-unavailable message;
symlinks show their target path without reading the target. Failures appear in
the panel instead of blocking the conversation. External diff, textconv, and
clean/process filters are disabled. Filter-managed files such as Git LFS files
therefore compare their workspace contents to the stored Git blob. The viewer
does not write the index.
