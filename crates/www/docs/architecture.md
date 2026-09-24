# Architecture

`src/pages/ulo/` contains product pages. `src/sites/ulo/` owns their layouts,
components, styles, and assets. Astro prerenders the guides and ordinary
pages. The changelog and installer are Worker endpoints.

`src/worker.ts` maps public paths to the internal `/ulo` prefix, redirects
prefixed URLs, applies security headers, and serves the product 404 page.
`astro.config.mjs` gives local development the same public paths.

`scripts/ulo/docs/docs.mjs` reads `../../docs/guides/` into
`src/generated/docs.json`. Pages, navigation, sitemap, and `llms.txt` read that
index. `scripts/ulo/docs/index-docs.mjs` builds Pagefind from the rendered
guides. There is no remote documentation checkout.

The installer imports `../../install.sh` through Vite as plain text. It ships
with the website build and returns a plain-text response with a five-minute cache.

The production contact endpoints use a service binding to Aro's existing
Worker. `src/shared/contact/` and `src/shared/email/` retain the local handlers
and validation tests for development. Operator and license details are shared
data, not part of the product rename.

The wordmark uses the supplied 150 by 30 SVG geometry, with ink inherited
from the theme. The share card and favicon use that same geometry.

See [terminal demo](terminal-demo.md) for the recorded video and its renderer.
