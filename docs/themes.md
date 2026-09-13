# Themes

A theme is a JSON file: `~/.e/themes/<name>.json`. Every name in
`/settings` → Theme comes from this directory plus the two built-ins
(`dark`, `light`) — a file named like a built-in replaces it.

## Format

```json
{
  "name": "mytheme",
  "vars": { "ink": 255, "dim": 245, "divider": 240, "shell": 71, "...": 0 },
  "colors": { "userMessageText": "ink", "border": "divider", "bashMode": "shell", "...": "" }
}
```

- `vars` maps a palette name to a 256-color index or `"#RRGGBB"` color.
- `colors` maps a UI token to a var name, index, or hex color; `""` means the terminal default.
- Start by copying a built-in: `e docs theme-dark` prints the dark theme's
  JSON verbatim; save it under a new name and edit.

Tokens you will most likely touch: `userMessageText` (the composer rail and
user text), `dim`, `border` (dividers), `muted`, `bashMode` (the `!` rail),
`accent`, and the `syntax*` family for code tinting. Unknown tokens are
ignored; missing tokens fall back to the terminal default — a partial theme
is valid.

Apply instantly with `/reload` (or pick it in `/settings`).

The full transcript reader uses `toolDetailRail` for its `│` rails. It defaults
to the terminal foreground, while the two-space indent and output use `dim`.
Its footer uses `userMessageText` for `┃` and `muted` for navigation.

## Diff review

The `/diff` pane has its own palette. Changing these tokens does not recolor
Markdown code blocks or inline tool summaries.

- `diffPaneBg`, `diffText`: pane background and source text.
- `diffAddedBg`, `diffRemovedBg`: full-row change backgrounds.
- `diffAddedWordBg`, `diffRemovedWordBg`: stronger backgrounds behind changed words.
- `diffSelectedBg`: selected source rows, with word changes still visible.
- `diffLineNumber`, `diffAdded`, `diffRemoved`: line numbers, signs, and counts.
- `diffSelectionText`: inline diff attachment marker.
- `diffSyntaxKeyword`, `diffSyntaxString`, `diffSyntaxNumber`,
  `diffSyntaxComment`, `diffSyntaxFunction`, `diffSyntaxType`: source syntax colors.

The bundled dark and light themes use separate diff colors. A partial custom
theme may leave these tokens unset to use terminal defaults.
