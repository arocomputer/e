# Working on the ulo website

This folder serves ulo from an Astro project deployed as the `ulo`
Cloudflare Worker (`src/worker.ts`). Start with
[README.md](README.md) and the [documentation index](docs/README.md). Read only
the guides relevant to the task, then check the implementation.

## Working rules

- Preserve unrelated changes and unpublished drafts. Never expose secrets.
- Keep changes simple. Use Astro's `src/pages`, with `src/components`,
  `src/layouts`, `src/styles`, and `src/data`. See [Architecture](docs/architecture.md).
- Preserve public routes and legal qualifiers. Contact is an email link;
  this service has no mail backend. See [Contact](docs/contact.md).
- Use existing Astro and React components and CSS. Add dependencies only for a concrete need.
- Name source files after their purpose. Use lowercase kebab-case, PascalCase
  components, and Python snake_case. Keep framework filenames unchanged.
- Comment components, modules, and helpers with their purpose and contract.
  Remove obsolete history and update documentation when a path or behavior changes.
- Run focused tests for behavior changes. Do not add tests for wording or framework
  behavior. Do not weaken assertions to make a regression pass.
- Format touched code. Run lint, typecheck, tests, and build before a release.
  See [Development](docs/development.md) for commands and browser checks.
- Inspect interface changes at desktop and mobile widths, including keyboard and
  reduced motion. Report actual checks and limitations.

## Branches and pull requests

- Before the first push, name the branch `<type>/<slug>`. Use the PR title's
  conventional type or scope plus two or three lowercase words, for example
  `bench/real-launches` or `fix/tool-tree-compaction`. Never push `main`, a bare
  SHA, or a vague generated name.
- Never open a PR unless the developer explicitly asks you to.

## Releases

Follow [Deployment](docs/deployment.md). Do not merge, publish, send mail, or
deploy without authorization. A merge to `main` deploys production through
`../../.github/workflows/www.yml`; `npm run deploy` does the same from a machine.

Keep the required check names `Build and test` and `Scan for secrets`. Worker
secrets are set with `wrangler secret put`, never committed or passed to the build.
