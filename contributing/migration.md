# ulo migration

The application, packages, and website now use `ulo`. The website source is
in `crates/www/`; its guides and installer come from this repository. Aro's
website and shared email delivery remain in `arocomputer/web`.

## Release order

Public package publication is paused during development. The repository has
been renamed to `arocomputer/ulo`; the sequence below applies when ready for
the first public ulo release. Enable publication as described in
[releases](releases.md#ship-production) only at that point.

1. Review and merge this migration. GitHub redirects the old repository URL;
   update local remotes to `arocomputer/ulo`.
2. Configure the `Production` GitHub environment with `CLOUDFLARE_API_TOKEN`
   and `CLOUDFLARE_ACCOUNT_ID`. The website deploys from
   `.github/workflows/www.yml`. The old cross-repository docs deploy token is
   no longer used.
3. Set `HOMEBREW_TAP_TOKEN`, scoped to `arocomputer/homebrew-tap` contents.
   The release workflow generates and publishes `Formula/ulo.rb` from four
   checksum-verified archives. The installation command is
   `brew install arocomputer/tap/ulo`.
   Publication records the Homebrew formula rename so existing installations
   can migrate through `brew update` and `brew upgrade`.
4. Bootstrap `@arocomputer/ulo` and its four native npm packages, then configure
   trusted publishing for the renamed repository and `release.yml`. Do the
   same for `@arocomputer/ulo-slack`. See [releases](releases.md).
   A missing npm package does not reserve its name. The bare `ulo` npm name
   has an unpublished history and requires an ownership check with npm.
5. Bump the application version beyond `0.0.2`, update the matching internal
   dependencies and lockfile, then publish a stable tag. Do not reuse an
   existing tag. The archives contain `ulo` and are named `ulo-<target>.tar.gz`.
6. Verify shell, Homebrew, npm, and bun installations on supported platforms.
   Verify `ulo --version`, `ulo doctor`, existing state, and package-manager
   update protection.
7. Deploy the prepared Aro website cleanup only after the new installer works.
   It removes the migrated product source, changes the product directory link,
   and redirects the old domain and `/e` bookmarks to `ulo.sh`.

## Cloudflare

The `ulo` Worker owns `ulo.sh` and `www.ulo.sh`. Wrangler provisions custom-domain
DNS and certificates. The `CONTACT_SERVICE` binding uses the existing `aro`
Worker, preserving its secrets, consent handling, and Durable Object limiter.
Keep that service available. Replicate the contact burst-limit rule on the
new zone and verify responses without sending a real email.

Keep the old product host on Aro while its redirect is deployed there. Do not
assign it to both Wrangler configurations. Existing Aro and mail DNS records
are independent of the new product domain.

## Existing users and integrations

`ulo` prefers `~/.ulo`, with `~/.ulo-dev` and `~/.ulo-pr` for local and PR
builds. If the corresponding directory is absent, it continues to use the
previous `.e` directory. Trusted workspace resources follow the same rule.
Directories are never merged, moved, or deleted automatically.

Move a directory explicitly after stopping running sessions, or set
`ULO_HOME` to an existing store. Rename environment variables from `E_*` to
`ULO_*`, command invocations to `ulo`, and Rust dependencies/imports to the
`ulo`, `ulo-core`, `ulo-tui`, `ulo-rpc`, and `ulo-sdk` names. Existing session,
configuration, and extension JSON formats retain their versions. Doctor JSON
now calls its home field `ulo_home`.

Crate registry publication is a separate step. The rename does not claim
that the new crate names have been published or reserved.
