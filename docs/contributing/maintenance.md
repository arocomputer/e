# Repository maintenance

The workflows in `.github/workflows/` keep maintenance visible without adding
external-site availability to the PR gate. CI's `ready` check remains the merge
gate; `./x workflows` runs syntax and security checks in its existing lint job.

## Scheduled checks

- Monday: CI audits Rust and Slack production dependencies and runs performance
  checks. Dependency updates are combined in one weekly Dependabot PR across
  Cargo, npm, GitHub Actions, and Docker; security updates can arrive separately.
- Tuesday: `links` runs `./x links` over root Markdown files and `docs/`. Relative
  links remain covered by `./x docs`. Live HTTP/HTTPS links are checked with
  bounded retries; reserved example domains and local/private addresses are
  excluded. The Markdown report is retained for fourteen days.
- Wednesday: `fuzz` exercises all three parser targets. Failed runs retain
  `fuzz-crashes-<target>` artifacts for fourteen days so crashes can be reproduced.

`scheduled-report` observes completed scheduled runs of `ci`, `fuzz`, and `links`.
It creates one bot-owned issue per failing workflow, updates that issue on later
failures, reopens it if needed, and closes it after a successful scheduled run.
Healthy runs with no open failure issue stay quiet. Manual, PR, fork, cancelled,
and outdated runs do not create issues. Watch repository issues to receive these
notifications. Issues receive no automatic labels.

The reporter uses a separate token with issue-write permission. It checks out
trusted default-branch code, reads run metadata, and never executes code or
downloads artifacts from the triggering run. These workflows become active after
they are merged to the default branch; a PR cannot enable a schedule by itself.

## Cache and artifact retention

Closing a PR triggers `cache-cleanup`. It snapshots all cache pages and deletes
only caches under that PR's `refs/pull/<number>/merge` ref. Active PR, branch, and
release caches are preserved. GitHub also evicts caches unused for seven days;
there is no periodic delete-all job.

The cleanup workflow uses trusted default-branch code and narrowly scoped
Actions write permission. Release jobs do not restore build caches. CI retains
its caches for speed. CI frames and reviewer binaries expire after seven days;
preview, release-transfer, link, and fuzz artifacts expire after fourteen days.
Published GitHub release assets are separate and are not cleaned up.

## Tool versions

`./x workflows` and `./x links` install checksum-verified actionlint, zizmor, and
lychee archives under `target/infra-tools/`. Their versions and Linux/macOS
archive hashes are pinned in `.github/infra-tools.json`. Update version and hashes
together. `./x audit` pins cargo-audit in `x`. These tool pins are maintained
deliberately; Dependabot does not update the JSON archive catalog.

## GitHub settings

Automatic deletion of merged branches is already enabled. The connected GitHub
app cannot inspect legacy branch protection or the repository's secret-scanning
settings. No repository rulesets were visible at review time; that does not
establish whether legacy branch protection is configured.

With an authenticated GitHub CLI account that has repository administration
access, inspect the requested settings locally:

```sh
./x repository-settings
```

Apply the desired settings explicitly:

```sh
./x repository-settings --apply
./x repository-settings
```

This enables merged-branch deletion, secret scanning, and secret-scanning push
protection, and binds `ready` to the GitHub Actions app on an up-to-date default branch.
Version tags can be created only by repository admins and cannot be moved or
deleted; the creation bypass does not bypass immutability. Before any settings
write, the latest Actions `ready` check must succeed on current main. A snapshot
of existing policy is saved under `target/settings/before.json`. Existing required
checks and their app identities are retained. Existing review requirements,
restrictions, and bypass rules are not replaced. If no legacy branch protection
exists, the command creates protection with the required status check enforced
for administrators too; maintainer
review remains project policy. Authentication or administration failures stop
before further changes. The default invocation is read-only and returns nonzero
for drift or inaccessible settings. GitHub feature availability can also block
a setting; inspect the GitHub error before retrying.

## CI selection and website integration

Maintenance workflows and reporting/settings tooling run the always-required
lint, guard, and workflow scanners without rebuilding the application. Pure
runtime test edits run both native test platforms without repeating UI and
packed-consumer checks. Unknown inputs select every layer. The selector emits
one job plan; both job conditions and `ready` use it, so an unexpected skip fails.

Published guide changes run `./x site`, which uses the exact website commit in
`.github/site-source.json` with this checkout's guides. Update that pin deliberately
when changing the site importer contract. `./x site --source /path/to/web` uses a
local renderer for development. The website's trusted main workflow checks for
merged guide changes twice an hour and deploys a build of the exact selected e
commit. That scheduled integration becomes active when its web changes merge.

`./x fuzz-check` compiles active targets when their code, lockfile, or core/TUI
source changes. The expanded audit identified and patched the fuzz workspace's
rustls TLS advisory (RUSTSEC-2026-0285) and replaced yanked chacha20 0.10.1.
