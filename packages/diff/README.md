# e-diff — Git review as an e extension

The review surface that started life inside `e`'s TUI, extracted: a standalone
binary that speaks [e's extension line protocol](../../docs/extensions.md)
(one JSONL request in, one response out over stdio). It is deliberately not a
dependency of the `e` binary — people who want `/diff` build and install it;
everyone else ships nothing extra.

`/diff` shows the workspace review as one block in the transcript: a title
with the file count and `+added/-removed` totals, then every patch painted by
e in its own diff grammar (the extension sends a unified diff through the
`show` surface; the host colours it). `/diff <path>` shows one file's patch. Reads are bounded and fail closed: no
Git index writes, no external diff/textconv/clean-process filters, no symlink
traversal toward files outside the repository, and no spawning of a `git`
that resolves through a relative PATH entry or lives inside the workspace.

## Install

```sh
cargo build --release -p e-diff
cp target/release/e-diff ~/.e/extensions/e-diff   # must be executable
```

Restart e or run `/reload`. `/diff` appears in the command picker as an
extension command. Remove the file to remove the feature.

## Configuration

`~/.e/settings.json` reaches the extension as `extensions_config`; see
[docs/diff.md](../../docs/diff.md) for the keys and the full comparison rules.

## Layout

- `src/diff.rs` — the Git scanner: bounded reads, numstat parsing, and the
  symlink-safe `open_in_root` traversal used for new-file previews.
- `src/diffpanel.rs` (+ `diffpanel/patch.rs`) — the review document: layout,
  word-level change marks, the mouse-driven pane renderer, and `document()` —
  the whole review as styled rows, which is what the command prints.
- `src/command.rs` — the review as a `show` object for the host.
- `src/main.rs` — the line protocol loop: manifest on `initialize`, review on
  `command`.
- `src/style.rs`, `src/frame.rs`, `src/theme_*.json` — the extension's own
  palette and framing, built on [`e-terminal`](../terminal).

The pane renderer is exercised by tests even though the current command
surface only prints documents; it is the shape a future host UI protocol
would reuse.

```sh
cargo test -p e-diff
```
