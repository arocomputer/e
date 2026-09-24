---
title: Themes
description: Recolor ulo with a theme JSON file.
order: 6
---

# Themes

A theme is a JSON file that sets ulo's colors. Put it at
`~/.ulo/themes/<name>.json`, or at `themes/<name>.json` in an installed
[package](../extend/packages.md).

The names in `/settings` → Theme come from these directories plus the two
built-ins, `dark` and `light`. A file named like a built-in replaces it. When
both the home and a package have one, the home's file comes first.

## Format

```json
{
  "name": "mytheme",
  "vars": { "ink": 255, "dim": 245, "divider": 240, "shell": 71, "...": 0 },
  "colors": { "userMessageText": "ink", "border": "divider", "bashMode": "shell", "...": "" }
}
```

- `vars` maps a palette name to a 256-color index or a `"#RRGGBB"` color.
- `colors` maps a UI token to a var name, an index, or a hex color. `""` means
  the terminal default.

To start, copy a built-in. `ulo docs theme-dark` prints the dark theme's JSON
verbatim. Save it under a new name and edit it.

Apply a theme instantly with `/reload`, or pick it in `/settings`.

## Tokens

These are the tokens you will most likely touch:

- `userMessageText`, for the composer rail and user text
- `dim`
- `border`, for dividers
- `muted`
- `bashMode`, for the `!` shell marker
- `accent`
- the `syntax*` family, for code tinting

ulo ignores unknown tokens. A missing token falls back to the terminal default,
so a partial theme is valid.

### Full transcript reader

The reader rails tool details with a `│` in the `muted` tone, and shows output
text dim. Its footer uses `userMessageText` for `┃` and `muted` for
navigation.

### Diff markers

Edit and write summary counts use these tokens. They also color the review's
diff markers.

| Terminal | Additions | Deletions |
| --- | --- | --- |
| Truecolor | `toolDiffAddedMarker` | `toolDiffRemovedMarker` |
| Other | `toolDiffAddedMarkerFallback` | `toolDiffRemovedMarkerFallback` |

By default additions are green and deletions are red. Labels and tree rails
stay neutral.
