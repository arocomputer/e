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
user text), `dim`, `border` (dividers), `muted`, `bashMode` (the `!` shell marker),
`accent`, and the `syntax*` family for code tinting. Unknown tokens are
ignored; missing tokens fall back to the terminal default — a partial theme
is valid.

Apply instantly with `/reload` (or pick it in `/settings`).

The full transcript reader rails tool details with a `│` in the theme's
`muted` tone and dim output text. Its footer uses `userMessageText` for `┃`
and `muted` for navigation.

## Diff review

The `/diff` command is the `packages/diff` extension, and its palette ships
inside the extension rather than in the host themes: the `diffPaneBg`,
`diffText`, `diffAddedBg`/`diffRemovedBg`, `diffSelectedBg`, `diffLineNumber`,
and `diffSyntax*` tokens live in `packages/diff/src/theme_{dark,light}.json`.
`~/.e/themes/` cannot recolor the extension's output today; the host's
`theme` setting selects which embedded palette `/diff` renders with.

Edit/write summary counts use `toolDiffAddedMarker` and `toolDiffRemovedMarker`
for truecolor terminals, or `toolDiffAddedMarkerFallback` and
`toolDiffRemovedMarkerFallback` otherwise. These also color the review's diff
markers. The defaults are green for additions and red for deletions; labels and
tree rails remain neutral.
