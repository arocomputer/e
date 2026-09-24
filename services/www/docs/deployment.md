# Deployment

The repository's `.github/workflows/www.yml` checks changes and deploys
`services/www/` on `main`. The Worker is named `ulo`. Its custom domains are
`ulo.sh` and `www.ulo.sh`; the latter redirects to the apex.

The GitHub `Production` environment needs `CLOUDFLARE_API_TOKEN` and the
`CLOUDFLARE_ACCOUNT_ID` variable. The token must allow Workers deployment,
KV access required by the Astro adapter, and management of the custom domains.
For an authorized local deploy, run `npm run deploy` from this folder.

No email keys or service bindings are needed. The `v2` Durable Object migration
removes the retired contact rate limiter from existing deployments. Keep its
migration history so Cloudflare can apply the change to the deployed Worker.

The Aro website redirects the old product domain and `/e` bookmarks to
`ulo.sh`. Keep that domain in only one Wrangler configuration. This service
preserves its own old `/ulo` bookmarks and unsubscribe URLs.
