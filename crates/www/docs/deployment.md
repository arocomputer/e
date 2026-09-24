# Deployment

`../../.github/workflows/www.yml` from this folder builds and checks the website
on pull requests, then deploys changes on `main`. The Worker is named `ulo`.
Its custom domains are `ulo.sh` and `www.ulo.sh`; the latter redirects to the apex.
Cloudflare provisions their DNS records and certificates on deployment.

The GitHub `Production` environment needs `CLOUDFLARE_API_TOKEN`, allowed to
edit Workers and custom domains in the Aro account, and the variable
`CLOUDFLARE_ACCOUNT_ID`. For an authorized local deploy, run `npm run deploy`.

## Shared contact delivery

The `CONTACT_SERVICE` service binding calls the existing `aro` Worker for the
three contact and unsubscribe API endpoints. Its signing keys, Resend
credentials, consent handling, and Durable Object rate limiter stay there.
The product Worker does not receive or copy those secrets. Unsubscribe
confirmation stays on `aro.computer`, which signs outgoing email links.

Apply the existing contact burst-limit policy to the new domain before moving
traffic. See [contact delivery](contact.md).

## Cutover

Publish the renamed CLI release before redirecting the old product domain.
`src/worker.ts` supports the old host, including prefixed bookmarks, but it
does not take ownership of that hostname on a routine deploy. Move the old
host only after the installer works, then remove its domain route from the
old repository's Wrangler configuration so a later Aro deploy cannot take it back.

The repository-wide [migration checklist](../../../contributing/migration.md)
tracks repository, package, and domain steps.
