# Releases and testing

## Run changes locally

Use Rust for local builds and Python 3.11 or newer for scenario and release tooling.
PR preview commands also require an authenticated GitHub CLI.

```sh
./x dev /path/to/project
./x scenario streaming
./x scenario tools
./x scenario cancellation
./x scenario long-output
./x scenario resume
```

`./x dev` builds the current checkout and runs it in the selected project.
Arguments after the project path go to e. Local builds use `~/.e-dev` and never
self-update. Set `E_HOME` to use another dedicated state directory.

Scenarios run the real terminal against the existing loopback fixture provider.
They use temporary settings, dummy credentials, and a disposable project; no
paid provider calls run. Streaming, long-output, and cancellation share the same
paced response so you can inspect scrolling or interrupt it. Tools enables a
synthetic shell command and leaves the workspace trust choice to you. Resume
reopens the saved conversation after the first terminal exits. State is removed
when the scenario command finishes.

## Release channels

GitHub titles use `X.Y.Z` for stable releases and `X.Y.Z · Beta N` or
`X.Y.Z · Dev N` for previews. Tags and package versions retain the full
channel, sequence, and commit identifier used by installers.

| Channel | Trigger | Executable | Default state |
| --- | --- | --- | --- |
| stable | `vX.Y.Z` tag on a commit reachable from main | `e` | `~/.e` |
| dev | Successful CI for code changes on main | `e-dev` | `~/.e-dev` |
| beta | Release workflow, action `beta`, selected main commit | `e-beta` | `~/.e-beta` |
| PR | Preview workflow, explicitly requested PR | `e-pr-NUMBER` | `~/.e-pr/COMMIT` |

`Cargo.toml` owns the base version. `scripts/release/identity.py` derives channel,
package tag, executable name, and preview version. `build.rs` embeds the workflow's
version, channel, and full source commit. Preview identities use
`X.Y.Z-dev.NUMBER.gCOMMIT` or `X.Y.Z-beta.NUMBER.gCOMMIT`; the sequence is the
Release workflow run number. PR builds use that workflow's run number.
`e --version --json` and `e doctor` report the build identity.

Dev and beta are GitHub prereleases and never become GitHub's latest stable
release. The `channel-dev` and `channel-beta` releases contain only a moving
`version.txt` pointer. The pointer advances after npm and brew both publish.
Versioned artifacts remain attached to their own release. Older retries cannot
move package tags, formulas, or channel pointers backward. PR and local builds
never self-update. Stable, dev, and beta self-updates stay in their own channel.

Each home owns its credentials, settings, sessions, and extensions. Sign in
separately in a new channel. State isolation does not isolate project edits;
use a disposable project or worktree when trying unfinished features.

## Install or switch channels

```sh
curl -fsSL https://e.intuitum.sh/install.sh | sh -s -- --channel beta
npm install -g @intuitums/e@beta
bun add -g @intuitums/e@beta
brew install intuitums/tap/e-beta
```

Use `dev` instead of `beta` for development builds. Stable remains the default:
no curl option, npm/bun `@latest`, or the `intuitums/tap/e` formula.
Curl and brew support side-by-side installations. npm and bun replace the
installed version of `@intuitums/e` when switching its tag. Package installs
carry channel-specific ownership markers; e directs updates to that manager.

To reinstall an older version with curl, pass `--version X.Y.Z`; a preview also
needs `--channel beta` or `--channel dev` and its full preview version. npm and
bun accept an exact package version after `@`. Use curl in a separate directory
for a historical brew build. An older curl build follows newer releases again
unless auto-update is disabled in its settings.

## Try a PR before merging

```sh
./x preview 123
# After its Preview run succeeds:
./x preview 123 --run RUN_ID
```

Requires `gh` authentication with access to Actions. The request returns
immediately; find the run with `gh run list --repo intuitums/e --workflow preview.yml`.
The installer verifies the artifact checksum and prints its source commit.
It installs `e-pr-123` under `~/.local/bin`, or `E_INSTALL_DIR`.
Artifacts expire after 14 days. The selected run stays pinned even if the PR
changes later; request a new run to test new commits. PR code is unreviewed and
can execute arbitrary code when built or run. Its workflow uses read-only
permissions, no publishing credentials, and no persisted checkout token.

## Select a beta and ship stable

1. Prepare the next base version in `Cargo.toml` and `Cargo.lock` on main.
2. Run Release with action `beta` and the chosen main commit. An empty commit
   selects current main. The workflow runs `./x check`, qualifies the build,
   builds all four platform archives, and publishes the channel installers.
3. Test that beta. Fixes produce another beta from a newer main commit.
4. Review the release notes, move Unreleased into `## X.Y.Z`, and add its date,
   title, introduction, and fixed groups. Create a fresh Unreleased section.
5. Qualify the final commit with `./x check` and `./x release-check vX.Y.Z`, then
   tag it `vX.Y.Z` and push the tag.

Stable recompiles with the stable identity. It is not a byte-for-byte rename of
the beta binary. Keep functional changes out of the final promotion commit;
if code changes, test another beta. The stable workflow checks the final commit
again. No permanent beta or production branch is required.

## Release notes and the website

Use `### New features`, `### Improvements`, and `### Fixes`, in that order;
omit empty groups. Keep `## X.Y.Z` exact for extraction. Put the date below it,
then one `###` release title and a short introduction. Bullets may wrap across
lines. Use inline code and bold for emphasis. Put **Upgrade:** instructions
first under Improvements and **Security:** fixes under Fixes.

The GitHub body comes from that version's section verbatim. The workflow
exports the same content as `release.json`, with its version and source commit.
The website reads that asset and GitHub's publication date, refreshing every
five minutes. It excludes drafts, prereleases, channel pointers, and Unreleased.
The first historical release predates the asset and remains a checked-in website
entry. New releases need no separate website copy or deployment.

## Verification and retrying publication

Release builds use the committed lockfile. Each release has four archives,
`checksums.txt`, a CycloneDX SBOM, and GitHub build-provenance attestations.
The workflow smoke-tests native binaries and the shell installer before
publication, then tests pinned and channel installs through the public website.

```sh
sha256sum -c checksums.txt --ignore-missing
gh attestation verify e-x86_64-unknown-linux-gnu.tar.gz --repo intuitums/e
```

On macOS use `shasum -a 256`. To retry a partial package publication, run Release
with action `retry` and the existing version tag. This downloads the existing
archives instead of rebuilding. For a draft whose builds finished, retry regenerates
checksums, the SBOM, and provenance in an isolated temporary directory, then
publishes the draft before updating packages. Incomplete drafts fail until all
four archives exist. npm compares the existing package's integrity;
a different tarball under the same version fails. Each registry can fail
independently; rerun failed publication after recovery. There is no transaction
across registries.

```sh
python3 -m unittest discover -s scripts/release -p 'test_*.py'
python3 -m unittest discover -s scripts/packaging -p 'test_*.py'
node --test scripts/packaging/publish-npm.test.mjs
scripts/packaging/smoke.sh
```

PR CI runs installer checks only when packaging, installer, identity, updater,
or workflow sources change. Docs-only main changes do not publish dev builds.
Release installation checks always run. Preview builds require an explicit request.

## Publishing credentials

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


The website installer at `https://e.intuitum.sh/install.sh` serves the maintained
script from main with a five-minute cache. Merge the channel-aware installer
before attempting the first channel release. No separate deployment is required
for each binary release. The stable homepage installation stays unchanged.
