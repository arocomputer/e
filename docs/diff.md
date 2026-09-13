# Diff review

Run `/diff` to open or close a live, read-only Git diff beside the conversation.
The conversation stays on the left. The right panel lists changed files and
added/removed line counts above the selected file's unified diff. Tests and
generated files remain visible.

The comparison is the current workspace versus `HEAD`, including staged and
unstaged changes together and non-ignored untracked files. It includes changes
made outside e. A repository without commits compares its current files to an
empty tree. Renames appear as a deletion and an addition. This version does not
provide session-only or branch-base comparisons, staging, or reverting.

The panel refreshes after tool completion and periodically while visible.
Only the selected file's patch loads. A refresh preserves the selected path
and scroll position, clamping them if the file changes or disappears. If the
patch changes while you are selecting lines, the selection clears rather than
attaching different text under the old selection.

## Navigation

- `Ctrl+D` switches focus between the conversation and the diff panel.
- In the file list, `Up`/`Down` or `k`/`j` select a file. `Enter` focuses its diff.
- In the diff, `Up`/`Down` or `k`/`j` move through lines. `PageUp`/`PageDown`
  scroll a page; `Home`/`End` move to the ends. `Left`/`Right` scroll horizontally.
- Hold `Shift` while moving through the diff to select lines. `Enter` attaches
  the selection, or the current line, to the draft and returns focus to it.
- Click a file to select it. Drag across diff rows to select lines, then press
  `Enter` to attach them. The mouse wheel navigates the area under the pointer.
- `Esc` returns from the diff to the file list, then closes the panel.
  Clicking the header's `×` also closes it.

An attachment contains a snapshot of the selected diff and its hunk header,
not a live file reference. Its dim `[Diff …]` marker owns that text. Deleting
or replacing the marker discards the snapshot. It never submits by itself.

The split needs 110 terminal columns by default. Below that width, the focused
pane fills the screen; `Ctrl+D` switches between review and the draft without
losing either. Review uses the alternate terminal screen so closing it restores
the normal transcript and scrollback. A terminal resize follows e's existing
transcript reflow behavior.

## Preferences

These keys in `~/.e/settings.json` apply when you next open the panel:

| Key | Default | Purpose |
| --- | --- | --- |
| `diff_min_width` | `110` | Minimum split width, at least 60 columns |
| `diff_width_percent` | `50` | Diff share of the terminal, between 30 and 70 percent |
| `diff_refresh_ms` | `1000` | Periodic refresh interval, at least 250 ms |
| `diff_title` | `Diff · workspace vs HEAD` | Header text |
| `diff_hint` | `↑↓ move · Enter · Ctrl+D focus · Esc back` | Status-row hint |

Colours use the active theme: `border` for dividers, `dim` for secondary text
and attachment markers, `userMessageText` for the selected file, and the
`toolDiffAddedMarker`/`toolDiffRemovedMarker` tokens and their terminal
fallbacks for additions and deletions.

Git reads have a five-second timeout and bounded output. Patches over 256 KiB
show a truncation notice. Binary files have no line counts. New files over
2 MiB or containing non-UTF-8/binary data show a preview-unavailable message;
symlinks show their target path without reading the target. Failures appear in
the panel instead of blocking the conversation. External diff, textconv, and
clean/process filters are disabled. Filter-managed files such as Git LFS files
therefore compare their workspace contents to the stored Git blob. The viewer
does not write the index.
