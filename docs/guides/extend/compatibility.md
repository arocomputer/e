---
title: Compatibility
description: The contracts e keeps stable across releases.
order: 4
---

# Compatibility

e is still pre-1.0. This page names the surfaces you can persist or build
against, so changes to them are deliberate, not accidental.

## Supported contracts

### CLI

Documented commands and exit statuses are user-facing. Before 1.0, an
incompatible change requires a changelog entry and migration guidance.

CLI one-shot commands return these exit statuses:

| Status | Meaning |
| --- | --- |
| `0` | The requested operation completed. |
| `1` | An operational or provider failure. |
| `2` | Invalid arguments, or an unknown requested resource. |

`e doctor` is a local-only diagnostic command. It returns 0 after producing
its report. It never turns provider reachability into a network side effect.

### Sessions

Session JSONL headers carry `format_version`. e reads these versions:

- Version 0, the unmarked pre-release format.
- Version 1.
- Version 2, which keeps response provenance and disjoint usage in an
  envelope outside replayable message content.

Readers reject a newer version with an actionable error instead of guessing.

### Configuration

Writes to `settings.json`, `auth.json`, and `trust.json` carry
`format_version: 1`.

- Readers accept unversioned files.
- Readers preserve unknown keys.
- Readers quarantine corrupt input before creating a replacement.
- An older e will not write over a file that carries a newer or invalid
  format version.

### Layout

`~/.e/layout.json` is documented in [Layout](../customize/layout.md). Its
keys are `panes`, `split_min`, `focus`, `banner`, `status.left`, and
`status.right`. Unknown keys are ignored, and a malformed file falls back to
the defaults.

### Packages

The `packages` list in `settings.json` holds source strings as typed. A
source is one of:

- `npm:name[@version]`
- `git:host/user/repo[@ref]`
- a git URL
- a directory path

An entry can also be an object that carries a `source` plus per-kind filter
lists: `extensions`, `skills`, `prompts`, and `themes`.

npm packages live under `~/.e/packages/npm/node_modules/<name>`. Git
packages live under `~/.e/packages/<host>/<path>`.

All of this is documented in [Packages](packages.md) and pinned by
`tests/fixtures/config/settings-v1-packages.json`. A reader that meets an
entry it cannot parse reports it and loads the rest.

### `e rpc`

The headless session protocol reports `protocol: 2` in `hello`. A line
without `method` is the version-1 one-shot request and keeps its flat
response.

Methods, parameters, result fields, and the `session` and `request` tags on
event lines become a supported contract once documented in
[Automation](../usage/automation.md). New methods and fields are additive
and do not change the protocol number. A change to an existing shape does.
`tests/fixtures/rpc/v2-requests.jsonl` pins the request shapes.

Optional parameters reject wrong JSON types instead of falling back to their
defaults, and `tests/fixtures/rpc/v2-invalid-requests.jsonl` pins those
refusals. Valid requests still speak protocol 2.

### Extensions

The extension JSONL protocol is versioned independently. e sends its
protocol number during `initialize`. Additive fields do not change the
number. Incompatible wire changes require a new protocol version.

Version 1 is documented in [Extensions](extensions.md). The families beyond
it are additive: `events`, `hooks`, `display`, `ui`, `session`, and
`shortcuts`. Each family is advertised in `capabilities`, declared in the
manifest, or initiated by the extension. A version-1 extension is never sent
a message it did not ask for.

A method name, event name, field, or result shape in those families is a
supported contract once documented.

## Compatibility fixtures

Compatibility fixtures under `tests/fixtures/` are release artifacts in
source form. Once committed for a release, they are not rewritten. Newer
readers must keep loading them or intentionally document the migration.

Regenerable caches such as `models-store.json` and `models-dev.json` are
internal. They are not a persisted compatibility contract.

## Session locks

Session sidecars now use OS-held locks. Stop older e processes before you
resume their sessions with the new writer. Writers that use PID locks and
writers that use OS locks must not open the same session concurrently.

Existing JSONL needs no migration. Empty `.lock` sidecars are expected. Do
not delete them.

## Error diagnostics

Provider failure diagnostics go to separate `<session-stem>.errors.jsonl`
files. This keeps message logs readable across their supported versions.

These sidecars carry their own `format_version: 1` and link records to
message IDs. You can remove them without changing conversation history.

Headless responses add an optional `error_details` object and keep the
`error` string.

## Home directory and file permissions

Each release channel has its own home directory:

| Channel | Home |
| --- | --- |
| Stable | `~/.e` |
| Dev and local | `~/.e-dev` |
| Beta | `~/.e-beta` |
| PR builds | `~/.e-pr/COMMIT` |

`E_HOME` overrides the channel default. Use a dedicated directory for
`E_HOME`. It is private application state, not a shared workspace. Files
copied outside that directory are not migrated.

On Unix, e creates its state directories with `0700` and session logs with
`0600`.

- Configuration writes and session creation or reopening also tighten the e
  home directory to `0700`. This protects older files underneath it without
  rewriting their contents.
- Reopening an older session sets its file to `0600`.
- Stricter owner permissions are preserved, including read-only directories.
- Credential staging files start at `0600`, before any secret is written.

## Redirects

Provider and OAuth endpoints must be final URLs. Authenticated requests no
longer follow HTTP redirects, including same-origin redirects. Update any
custom gateway URL that relied on one.

Release asset downloads still follow redirects. They do so without provider
credentials, and they reject HTTPS-to-HTTP downgrades.

## File writes

The filesystem `write` and `edit` tools stage and sync content before
committing it.

- Existing files are updated through their original inode. This preserves
  symlink targets, hard-link aliases, ACLs, and extended attributes.
- A staging failure leaves the original intact. An I/O failure during the
  in-place copy can leave a partial update.
- New files are published without overwriting a concurrent creator.
- On Unix, the parent directory is synced before success is reported.
- Unix writes also check that the target still names the opened inode,
  before and after copying. A detected external replacement fails the write,
  so the caller can reread and retry. External writers still need their own
  coordination.

On filesystems without hard links, a failed new-file copy removes its
partial target when that target still identifies the created file. Freshness
checks allow a confirmed deletion but fail closed on other metadata errors.

## Tool arguments

Tool integer arguments accept:

- JSON unsigned integers
- integral JSON floats below 2^64
- decimal integer strings within the u64 range

Out-of-range values fail validation instead of saturating.

A read line larger than the output window is reported as an error with an
offset to skip it. It is never returned as a complete truncated line.

Files and saved sessions need no migration.

## Not a supported contract

The Cargo library target lets the binary, the integration tests, and the
`sdk/` package share code. Its public Rust items are not a stable
third-party API in themselves.

The supported Rust SDK is the separate `intuitums-e-sdk` crate in `sdk/`.
See [SDK](sdk.md). The API it consumes is its documented contract. It
follows semantic versioning from its first published release. Before 1.0, a
breaking change moves the minor version and is named in the changelog.

## Change process

A change to a supported contract needs all of the following in one pull
request:

1. A compatibility fixture or contract test.
2. Migration behavior for existing user data or extensions.
3. Documentation and a changelog entry.
4. Updated contract documentation in the relevant guide.
