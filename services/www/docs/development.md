# Development

Run commands from `services/www/` with Node.js 22 or newer.

```sh
npm ci
npm run dev
```

The dev server serves ulo at <http://localhost:4321>. To check Worker routing,
build and start the preview:

```sh
npm run preview
npm run test:routes
```

Run the routing tests in another terminal. To use an existing build on a
different port, run `node scripts/preview.mjs --port 8790`, then
`ROUTING_TEST_ORIGIN=http://localhost:8790 npm run test:routes`.

## Documentation

Edit `../../docs/guides/`. `npm run docs` generates the page data and also runs
before dev, typecheck, and build. `ULO_DOCS_PATH` can select another source
checkout. `ULO_DOCS_REF` changes GitHub source links, not the imported files.

The `getting-started` guide becomes `/docs`; groups and all other routes come
from the guides' front matter. Build after editing guides to regenerate the
Pagefind search index. The importer rejects malformed metadata, duplicate
topics, and links to missing section anchors.

## Checks

```sh
npm run format:check
npm run lint
npm run typecheck
npm test
npm run test:recording
npm audit --audit-level=high
npm run build
```

The Worker hot-reload regression runs against a development server and checks
its server log, including errors hidden by client hydration:

```sh
npm run dev > /tmp/ulo-dev.log 2>&1 &
DEV_TEST_LOG=/tmp/ulo-dev.log npm run test:dev
```

Wait for Astro's ready message before running the test. Use `DEV_TEST_ORIGIN`
when the server uses a different port. The test touches two Worker modules
without changing their contents. CI runs it automatically.

Run `npm run test:routes` against a fresh preview after the build. The suite
checks public routes, old-host redirects, canonical URLs, assets, and the
installer's exact bytes. No email credentials are needed for local checks.

## Browser review

Inspect desktop and 390px layouts. Check the wordmark, mobile menu,
development notice, source-build link, FAQ, and footer.
Check docs navigation, search results and empty states, code copy, section
anchors, and keyboard focus restoration. The sidebar must reach its final
item at short viewport heights without scrolling the page behind it.

The terminal demo must load, pause, seek, and replay. Reduced motion disables
autoplay; background tabs and offscreen playback pause. Its export process is
documented in [terminal demo](terminal-demo.md).

Check that Contact opens the intended email address, the page has no form,
and old unsubscribe links redirect to their original service. Do not send
real messages during browser review.

## Generated files

`dist/`, `.astro/`, `.wrangler/`, `src/generated/`, and
`public/pagefind/` are ignored build output. The video, poster, social
card, and SVG assets are committed. Run `npm run social` after changing the
wordmark or share-card layout.

Published stable GitHub releases supply `/changelog`. Drafts, prereleases,
and `Unreleased` stay hidden. A failed refresh retains the last good cached
list for up to a day. With no available releases, the page points to source builds.
