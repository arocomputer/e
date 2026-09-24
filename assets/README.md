# ulo artwork

`logo-dark.svg` is the supplied 150 by 30 white wordmark. `logo.svg` uses the
same paths in black. `icon.svg` follows the viewer's color scheme. Scale the
SVGs without changing their proportions or spacing.

The website uses the same paths in
`services/www/src/components/logo.tsx` and its favicon. The wordmark
inherits the surrounding ink color. `npm run social` in `services/www/`
regenerates the share card from those paths.

## Themes

The bundled terminal palettes live in `crates/core/themes/`. The rename
changes the banner's text, not the palettes or transcript layout.

## README screenshot

`readme.png` shows the website's recorded editing session with a compact
window frame from `readme-window.html`. The source video and poster live in
`services/www/public/`. See the website's
[recording guide](../services/www/docs/terminal-demo.md) to regenerate them.

Extract a frame with FFmpeg, serve this repository locally, then open
`assets/readme-window.html` and export its canvas as a PNG. The canvas keeps
transparent rounded corners; a browser screenshot flattens them.
