# e artwork

`logo.svg` is the 110 by 50 split-arc mark in black for the README header.
`logo-dark.svg` uses the same paths in white. `icon.svg` is the existing app
icon and follows the viewer's color scheme. Scale the SVGs without changing
their proportions or spacing.

The website in `arocomputer/web` uses the same mark in
`src/sites/e/components/logo.tsx` and `public/e/icon.svg`. Run `npm run social`
there after changing the share card.

## Themes

The bundled terminal palettes live in `crates/core/themes/`. The rename
changes the banner's text, not the palettes or transcript layout.

## README screenshot

`readme.png` shows the website's recorded editing session with a compact
window frame from `readme-window.html`. The source video and poster live in
`arocomputer/web/public/e/`. See that repository's
`docs/terminal-demo.md` to regenerate them.

Extract a frame with FFmpeg, serve this repository locally, then open
`assets/readme-window.html` and export its canvas as a PNG. The canvas keeps
transparent rounded corners; a browser screenshot flattens them.
