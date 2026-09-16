# e artwork

- [icon.svg](icon.svg): the light mark on a dark rounded square, matching the
  favicon on e.intuitum.sh.
- [logo.svg](logo.svg): the standalone three-rectangle mark in `#252525` on a
  transparent background. Set its fill to `currentColor` when embedding inline
  if it should inherit the surrounding text color.
- [logo-dark.svg](logo-dark.svg): the same mark in `#fafafa`, for dark
  backgrounds where inline color is not available — the README's
  `<picture>` switches to it under `prefers-color-scheme: dark`.

Keep the proportions and spacing intact. Scale the SVGs rather than redrawing
the rectangles. The website copies live in the intuitum-sh repository at
`public/e/icon.svg` and `src/app/(sub)/e/brand-mark.tsx`; keep them aligned when
changing the artwork.

## Themes

The two bundled palettes live in `crates/core/themes/`, beside the core that
embeds them for `e docs theme-dark` and hands them to the terminal frontend.

## README screenshot

[readme.png](readme.png) shows e reading and editing `slug.py`, then running
three Python tests. It uses the built-in dark theme at 100 columns by 26 rows.
The session ran in an isolated example project with scripted local provider
responses. The file operations and test command were executed by e.

[readme.ansi.gz](readme.ansi.gz) is the original PTY capture from
`scripts/ptycap.py`, replayed with `scripts/term.py`. The screenshot preserves
the terminal cells and colors, with Menlo text and a window frame added for
the README. It contains no real provider credentials or private project data.

Inspect the captured terminal after setting up `./x ui`:

```sh
gzip -dc assets/readme.ansi.gz > /tmp/e-readme.ansi
target/ui-env/bin/python scripts/term.py /tmp/e-readme.ansi 100 26
```
