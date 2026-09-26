# Return to e

The product is named `e` again. The command, Rust crates, npm packages,
Homebrew formula, and release archives use that name. The repository is
`arocomputer/e`, and the website lives in `arocomputer/web` at
`https://aro.computer/e`.

Public package publication is paused. Before the next public release, verify
that the `e` package names and publication settings are available, update any
package ownership records, and follow [releases](releases.md).

Existing `~/.e` state takes precedence. If it does not exist, `e` reads the
corresponding `~/.ulo` directory, including `-dev` and `-pr` channel homes.
Workspace resources follow the same rule for `.e` and `.ulo`. No store is
moved or merged automatically. Use `E_HOME` to select a specific store, and
update integrations that set `ULO_HOME` or launch `ulo`.

The site in `arocomputer/web` imports guides from this repository. Merge the
product code first, then deploy the `web` repository so `aro.computer/e` is
live. Finally run the `Legacy domain redirect` workflow on `main` to move
`ulo.sh` and `www.ulo.sh`. Deploying that redirect before the new site would
create a loop with the current Aro Worker. The old `e.aro.computer` host
redirects to `aro.computer/e` as part of the website deployment.
