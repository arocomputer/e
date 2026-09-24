# ulo website

The Astro site at [ulo.sh](https://ulo.sh), deployed as the `ulo` Cloudflare
Worker. It is a standalone service with native Astro routes and a direct
email contact link.

## Develop

Use Node.js 22 or newer. From this folder:

```sh
npm ci
npm run dev
```

Open <http://localhost:4321>. `npm run preview` builds and serves the Worker
at <http://localhost:8787>, including redirects and security headers.

Guides come from `../../docs/guides/`. The installer comes from
`../../install.sh`. Both are built from the same checkout as the website.

```sh
npm run format:check
npm run lint
npm run typecheck
npm test
npm run test:recording
npm run build
```

Run `npm run test:routes` with the preview server running. See
[development](docs/development.md), [deployment](docs/deployment.md), and
[contact](docs/contact.md) for details.

This website retains its [license](LICENSE). The Rust application has the
repository's MIT license.
