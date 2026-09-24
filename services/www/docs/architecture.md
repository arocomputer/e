# Architecture

This is one Astro website. Routes live directly in `src/pages/`; their URLs
match the filesystem. Components, layouts, styles, data, and artwork live in
the corresponding folders under `src/`. Static files live in `public/`.
There is no site prefix. A small Vite middleware normalizes Astro's absolute
module URLs during development; it does not rewrite page routes.

`src/worker.ts` adds security headers and redirects existing bookmarks. It
passes ordinary requests directly to Astro and serves the site's 404 page
with a 404 status. Contact is a direct email link, not another service.

The Worker imports the renderer per request so development hot reload cannot
retain a renderer from an older React module generation. The regression under
`tests/development/` exercises overlapping file changes.

The `satori` dependency currently pins an affected `fflate` version. A scoped
npm override selects `0.8.3`; the social-card renderer produces the same image.
Remove the override when Satori's own dependency includes the fix.

`scripts/docs/docs.mjs` reads `../../docs/guides/` into
`src/generated/docs.json`. Pages, navigation, sitemap, and `llms.txt` use that
index. `scripts/docs/index-docs.mjs` builds Pagefind from the rendered guides
at their canonical `/docs` URLs.

The installer imports `../../install.sh` as plain text and ships with the
website build. The homepage points to source builds while public packages
are paused.

`scripts/social/` renders the share card. `scripts/recording/` captures the
terminal, and `scripts/video/` exports its video. See
[terminal demo](terminal-demo.md) for the process.
