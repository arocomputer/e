---
title: Themes
description: Recolor e with a theme JSON file.
order: 6
---

# Themes

A theme is a JSON file that sets every color e draws. Read this guide to make
your own palette or adjust a built-in one.

## Make a theme

1. Print the built-in dark theme and save it under a new name:

   ```sh
   mkdir -p ~/.e/themes
   e docs theme-dark > ~/.e/themes/mytheme.json
   ```

   `e docs theme-light` prints the light one.
2. Edit the colors.
3. Pick `mytheme` under Theme in `/settings`, or set `"theme": "mytheme"` in
   [settings](settings.md#interface) and run `/reload`.

The file stem is the theme's name. `/settings` lists `auto`, `light`,
`dark`, and every theme file it finds.

## Format

```json
{
  "vars": { "ink": 255, "dim": 245, "divider": 240, "shell": 71 },
  "colors": { "userMessageText": "ink", "border": "divider", "bashMode": "shell", "text": "" }
}
```

- `vars` names palette values: a 256-color index (`0` to `255`) or a
  `"#RRGGBB"` color.
- `colors` maps a UI token to a var name, an index, a `"#RRGGBB"` color, or
  `""` for the terminal's default color.

Both blocks are required; a file missing either one, or that is not valid
JSON, is ignored. Within `colors`, a token that is missing or has an invalid
value uses the terminal's default color, so a partial theme is valid.
Unknown tokens are ignored. The built-in files also carry a `name` key, which
e does not read.

## Where themes come from

| Location | Wins over |
| --- | --- |
| `~/.e/themes/<name>.json` | everything else |
| `themes/<name>.json` in an installed [package](../extend/packages.md) | the built-ins |
| the built-in `dark` and `light` | nothing |

A file named `dark.json` or `light.json` replaces that built-in, including
when `auto` picks it. `auto` follows the terminal's background and uses
`dark` when it cannot tell.

## Tokens

`e docs theme-dark` lists every token with its value. e itself draws with
these:

| Token | What e draws with it |
| --- | --- |
| `accent` | The first status-row segment and the activity dot. |
| `userMessageText` | Your messages, the composer's `┃` rail, and panel titles. |
| `muted` | Secondary text, such as the rest of the status row. |
| `dim` | Hints, unselected picker rows, pasted-text markers, and markdown rules and quote rails. |
| `border` | Panel dividers. |
| `thinkingText` | Expanded reasoning. |
| `success`, `warning`, `error` | The recovered notice, retries and cancelled tools, and errors and failed tools. |
| `customMessageText`, `customMessageLabel` | Finished tool markers, system notices, and extension messages. |
| `attachmentText` | The diff attachment marker in the composer. |
| `mdCode` | Inline code in markdown. |
| `syntaxKeyword`, `syntaxString`, `syntaxNumber`, `syntaxComment` | Code highlighting. |
| `bashMode` | The `!` shell-command marker in the composer. |
| the diff marker tokens | See [diff markers](#diff-markers). |

The other tokens in the built-in files are not drawn by e itself.
Extensions can paint text in any token by name, so keep them if an
extension you use relies on them.

In the transcript reader, tool details hang from a `│` rail in `muted`, and
their output is `dim`. The reader's footer uses `userMessageText` for its `┃`
and `muted` for the navigation text.

### Diff markers

The added and removed counts in edit and write summaries, and the diff
markers in the transcript reader, use these tokens. e picks the truecolor
pair when `COLORTERM` is `truecolor` or `24bit`.

| Terminal | Additions | Deletions |
| --- | --- | --- |
| Truecolor | `toolDiffAddedMarker` | `toolDiffRemovedMarker` |
| Other | `toolDiffAddedMarkerFallback` | `toolDiffRemovedMarkerFallback` |

By default additions are green and deletions are red. Labels and tree rails
stay neutral.
