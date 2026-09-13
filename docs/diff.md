# Diff review (extension)

`/diff` is not part of the e binary. It ships as e's first packaged extension,
`packages/diff`, a standalone Rust program that speaks e's extension line
protocol (see `docs/extensions.md`). Install it and `/diff` appears beside the
built-in commands in the picker; remove the file and nothing about it remains.

The review is read-only and never writes the Git index. It compares the
current workspace against `HEAD`: staged and unstaged changes together, plus
non-ignored untracked files, including changes made outside e. A repository
without commits compares its files to an empty tree. Renames appear as a
deletion and an addition. Session-only or branch-base comparisons, staging,
and reverting are not offered.

## Usage

- `/diff` prints the continuous review document into the transcript: a
  file-count header with `+added/-removed` totals, one summary row per changed
  file, then each file's heading and patch — line numbers, syntax colors,
  word-level change backgrounds, and wrapped long rows, with hunk headers
  hidden.
- `/diff <path>` prints one file's patch alone.
- Rows carry the extension's own palette (below); the transcript re-wraps them
  to the terminal width.

## Build and install

```sh
cargo build --release -p e-diff
cp target/release/e-diff ~/.e/extensions/e-diff
```

Restart e (or run `/reload`). The extension needs a `git` executable on an
absolute PATH entry outside the workspace; system locations are used as a
fallback. Relative PATH entries and workspace-supplied binaries are never run.

## Preferences

Keys in `~/.e/settings.json` reach the extension through the protocol's
`extensions_config`; they apply on the next `/diff`:

| Key | Default | Purpose |
| --- | --- | --- |
| `theme` | host default | `"light"` selects the extension's light palette |
| `diff_text_width` | `78` | Transcript row width, 40 to 120 columns |
| `diff_min_width` | `110` | Split width for the live pane design (library only) |
| `diff_width_percent` | `40` | Pane share (library only) |
| `diff_refresh_ms` | `1000` | Pane refresh interval (library only) |
| `diff_title` | `{count} {files} changed` | Header, with file count and singular/plural noun |
| `diff_selection_label` | `⧉ {count} {lines} from diff` | Attachment label the pane produces on selection (library only) |

## Safety limits

Git reads have a five-second timeout and bounded output. Each review includes
at most 128 patches and 4 MiB of patch text, and stops starting patches after
five seconds; omitted files are reported at the end of the document. Patches
over 256 KiB show a truncation notice. Binary files have no line counts. New
files over 2 MiB or containing non-UTF-8/binary data show a
preview-unavailable message. Every path component between the repository root
and a read file is opened without following symlinks, so a symlinked directory
reports an error rather than leaking its target's contents; top-level symlinks
show their target path, never the target's text. External diff, textconv, and
clean/process filters are disabled, so filter-managed files (Git LFS included)
compare workspace contents to the stored Git blob.

## The library

`packages/diff` keeps more than the command uses today: the continuous pane's
mouse navigation, drag-to-attach selection, and viewport rendering are library
code with tests, waiting for a host UI protocol that could surface them again.
`packages/terminal` holds the styling primitives (`e-terminal`) shared by e and
the extension, so the review renders with the same theme code as the host.
