# e-terminal — shared terminal primitives

The styling vocabulary `e` renders with, published so extensions can render
like the host without embedding another copy of it: the `Theme` palette
loader, ANSI-aware width/clipping/wrapping text helpers, syntax highlighting,
and the panel row protocol. `e`'s own `tui::theme`, `tui::text`, and
`tui::highlight` paths are thin host-side wrappers over this crate; the
difference is only who owns the palette file.

Keep this surface small. It grows when an extension needs what the host
already has, not in anticipation.

```sh
cargo test -p e-terminal
```
