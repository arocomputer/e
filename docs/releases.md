# Releases and verification

A tag `vX.Y.Z` is publishable only when it exactly matches the user-facing
`VERSION` in `src/lib.rs` and the `version` in `Cargo.toml`, has a nonempty
`## X.Y.Z` section in `CHANGELOG.md`, and passes the complete repository
contract. Release jobs build with the committed lockfile and smoke-test the
native binary and installer before publication.

## Writing release notes

Use the same layout as [fx's changelog](https://github.com/vercel-labs/fx/blob/main/CHANGELOG.md):

- Open with a short bold paragraph about what users can now do.
- Group changes under `### Breaking changes`, `### New features`,
  `### Improvements`, `### Bug fixes`, and `### Security`, in that order.
  Omit empty groups.
- Write short, flat bullets about behavior. Keep migration instructions and
  important limits; leave implementation inventories and test reports in PRs.
- Add new work to the matching group under `## Unreleased`. Do not prepend
  another chronological batch of notes.

The GitHub release uses the version section verbatim, without its heading or
an appended install block. Installation belongs in the README; artifact
verification stays below. Qualification rejects missing, empty, or duplicate
version sections rather than publishing fallback text.

## Cutting a release

1. Review `Unreleased` against the changes actually shipping. Do not include
   unreleased work when editing an older release's notes.
2. Rename `## Unreleased` to `## X.Y.Z` and open a new empty `## Unreleased`
   above it. Version headings do not include dates.
3. Bump `VERSION` in `src/lib.rs` and `version` in `Cargo.toml` to `X.Y.Z`,
   updating `Cargo.lock` as needed.
4. Preview the release body and qualify the candidate:

   ```sh
   ./scripts/release-notes.sh vX.Y.Z < CHANGELOG.md
   ./x check
   ./x release-check vX.Y.Z
   ```

5. Commit, then tag that commit `vX.Y.Z` and push the tag. The workflow creates
   a draft, uploads and verifies artifacts, then publishes it.

## Verifying downloads

Each release contains four binary archives, `checksums.txt`, and a CycloneDX
`e-sbom.cdx.json`. GitHub generates signed build-provenance attestations for
the artifacts. Given a downloaded archive:

```sh
sha256sum -c checksums.txt --ignore-missing
gh attestation verify e-x86_64-unknown-linux-gnu.tar.gz \
  --repo intuitums/e
```

On macOS use `shasum -a 256` to compare the archive with the corresponding
line in `checksums.txt`. A checksum detects corruption; provenance verifies
that GitHub Actions built the artifact from this repository's release
workflow.

## Homebrew, npm, and bun

A published stable release starts the Homebrew and npm jobs in `release.yml`.
They verify all four archives against `checksums.txt`, then generate the formula
and npm packages from that tag. The npm package has platform-specific optional
dependencies and a shell launcher. It works with npm and bun without lifecycle
scripts or a JavaScript runtime at launch.

The jobs publish `intuitums/homebrew-tap` and these public npm packages:

- `@intuitums/e`
- `@intuitums/e-darwin-arm64`
- `@intuitums/e-darwin-x64`
- `@intuitums/e-linux-arm64`
- `@intuitums/e-linux-x64`

Homebrew uses `HOMEBREW_TAP_TOKEN`, a fine-grained token limited to Contents
read/write on `intuitums/homebrew-tap`. Deploy keys are disabled by repository
policy. Renew the token before its expiry.
npm uses trusted publishing. Configure each package for GitHub organization
`intuitums`, repository `e`, workflow `release.yml`, with direct publishing
allowed. Use Node 24 with npm 11.5.1 or newer. The first publication needs an npm
account authorized for the scope. For that first release, store a publishing token
with permission to create packages under `@intuitums` as `NPM_BOOTSTRAP_TOKEN`
in `intuitums/e`. Unattended publishing requires a token that can bypass 2FA.
The npm job uses it as a fallback until trusted publishing is configured.

After the first publication, use an interactive npm login with 2FA enabled and
npm 11.15.0 or newer to configure the five packages:

```sh
for package in e e-darwin-arm64 e-darwin-x64 e-linux-arm64 e-linux-x64; do
  npm trust github "@intuitums/$package" \
    --repository intuitums/e --file release.yml --allow-publish --yes
  sleep 2
done
```

Complete npm's browser authentication when prompted. An API token that bypasses
2FA cannot configure trust relationships. Verify each package with `npm trust
list @intuitums/<package>`, then delete `NPM_BOOTSTRAP_TOKEN` from GitHub.
Subsequent releases need no npm token. The token in 1Password can remain available
for separately authorized manual publishing.

To retry package publication, manually run the Release workflow with the existing
stable tag. This skips binary builds and downloads the already published archives.
Retries compare npm integrity before accepting an existing version. Both channels
refuse to move their latest version backward. A registry outage fails its job;
rerun the failed job after service recovers. There is no cross-registry atomic
transaction.

Users install and update through the same channel:

| Channel | Install | Update |
| --- | --- | --- |
| curl | `curl -fsSL https://e.intuitum.sh/install.sh` piped to `sh` | `e update` |
| npm | `npm install -g @intuitums/e` | `npm install -g @intuitums/e@latest` |
| bun | `bun add -g @intuitums/e` | `bun add -g @intuitums/e@latest` |
| brew | `brew install intuitums/tap/e` | `brew update`, then `brew upgrade intuitums/tap/e` |

Run `e --version` to verify the installed version. Binary packages support macOS
and glibc Linux on ARM64 and x86-64. The website setup guide covers initial
installation and connecting a model at `https://e.intuitum.sh/docs`.

Package installers place `.e-install-method` beside the executable. Both automatic
and manual self-update stop before network access when that marker exists;
`e update` tells users to use their package manager. The first packaged release
must include this guard. Packaging rejects v0.0.1, which predates it.

To check packaging locally:

```sh
python3 -m unittest discover -s scripts/packaging -p 'test_*.py'
node --test scripts/packaging/publish-npm.test.mjs
scripts/packaging/smoke.sh
```

The smoke check installs temporary npm and bun packages with scripts disabled.
It leaves the user's global installations alone. CI runs it on macOS and Linux
when package sources, packaging scripts, the shell installer, package ownership
code/tests, the license, release qualification, or CI/release workflows change.
Other PRs and main-branch pushes skip these jobs. Renames check both paths; a
failed PR file lookup runs the jobs. Release installation checks always run.

The public shell installer URL is `https://e.intuitum.sh/install.sh`. The website
serves the maintained repository script with a five-minute cache. Release build
checks use local archives before publication. After publication, a separate job
downloads the installer through the website and checks the exact release version.
It then installs into another temporary directory without `E_RELEASE_BASE`,
checking the default download path against GitHub's latest published release.
Deploy the website endpoint before enabling these checks.
