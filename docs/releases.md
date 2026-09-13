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
