# e artwork

- [icon.svg](icon.svg): the light mark on a dark rounded square, matching the
  favicon on e.intuitum.sh.
- [logo.svg](logo.svg): the standalone three-rectangle mark in `#252525` on a
  transparent background. Set its fill to `currentColor` when embedding inline
  if it should inherit the surrounding text color.

Keep the proportions and spacing intact. Scale the SVGs rather than redrawing
the rectangles. The website copies live in the intuitum-sh repository at
`public/e/icon.svg` and `src/app/(sub)/e/brand-mark.tsx`; keep them aligned when
changing the artwork.

## Themes

`themes/dark.json` and `themes/light.json` are the two bundled palettes,
compiled into the binary by `src/tui/paint/theme.rs` and served verbatim by
`e docs theme-dark` / `theme-light`. They live here, not under `src/tui/`, so
the terminal-free core can embed them without naming the frontend.
