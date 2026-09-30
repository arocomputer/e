#!/bin/sh
# One repository entry point. Keep CI, contributor docs, and local checks on
# the same commands so no environment has a private definition of "green".
set -eu
cd "$(dirname "$0")"

usage() {
  echo "usage: ./x [dev|scenario|preview|hooks|check|fmt|lint|test|crates|docs|guard|packages|channels|audit-channels|container|ui|bench|audit|sbom|release-check] [args...]" >&2
  exit 2
}

command=${1:-check}
if [ "$#" -gt 0 ]; then
  shift
fi

case "$command" in
  dev)
    unset E_BUILD_VERSION E_BUILD_CHANNEL E_BUILD_COMMIT
    project=${1:-$PWD}
    if [ "$#" -gt 0 ]; then shift; fi
    project=$(CDPATH= cd "$project" && pwd)
    cargo build --locked
    binary="$PWD/target/debug/e"
    cd "$project"
    exec "$binary" "$@"
    ;;
  scenario)
    cargo build --locked
    exec python3 scripts/scenario.py "$@"
    ;;
  preview)
    exec python3 scripts/release/preview.py "$@"
    ;;
  hooks)
    [ "$#" -eq 0 ] || usage
    exec python3 scripts/hooks/install.py
    ;;
  # CI groups these commands into jobs; local checks use the same commands.
  check)
    [ "$#" -eq 0 ] || usage
    ./x fmt --check
    ./x lint
    ./x test
    ./x crates
    ./x guard
    ;;
  test)
    # Every failing suite in one run: without this, the first of 46 test
    # binaries stops the rest and a red pull request reports one problem.
    cargo test --workspace --locked --no-fail-fast "$@"
    ;;
  crates)
    [ "$#" -eq 0 ] || usage
    # Cargo's temporary publication registry caches crates by version. Reusing
    # it can verify old source when an unreleased version has not changed.
    # Give each check a fresh registry and build directory, including in CI.
    CARGO_TARGET_DIR=$(mktemp -d "${TMPDIR:-/tmp}/e-crates.XXXXXX")
    export CARGO_TARGET_DIR
    trap 'rm -rf "$CARGO_TARGET_DIR"' EXIT HUP INT TERM
    # Only core and SDK publish to crates.io. The application frontends ship
    # in the binary and may depend on pinned Git libraries. Their workspace
    # builds, tests, and release binary checks run separately. The SDK consumer
    # compiles both published crates from their normalized archives.
    cargo publish --dry-run --locked --allow-dirty \
      -p e-core
    python3 scripts/check-sdk.py
    ;;
  docs)
    [ "$#" -eq 0 ] || usage
    # The guides are a published contract: front matter, one group README per
    # folder, unique topics, and every relative link resolving.
    cargo test --locked --test docs
    ;;
  packages)
    [ "$#" -eq 0 ] || usage
    # Installer and package contents: npm and Homebrew packaging, the platform
    # launchers, and an install through each.
    python3 -m unittest discover -s scripts/packaging -p 'test_*.py'
    node --test scripts/packaging/publish-npm.test.mjs
    scripts/packaging/smoke.sh
    ;;
  channels)
    [ "$#" -eq 0 ] || usage
    # The reference channels, which are consumers of `e rpc` rather than part
    # of the binary.
    (cd services/slack && npm ci --no-fund --no-audit && npm run typecheck && npm test)
    python3 -m unittest discover -s services/github -p 'test_*.py'
    ;;
  audit-channels)
    [ "$#" -eq 0 ] || usage
    (cd services/slack && npm audit --omit=dev)
    ;;
  container)
    # The check builds without a published release, so it installs a stub `e`.
    # The release workflow builds the real image with the release it published.
    docker build "$@" --tag e-slack --build-arg E_RELEASE_STUB=1 services/slack
    docker run --rm --entrypoint sh e-slack -ec '
      test "$(id -u)" -ne 0
      test "$(stat -c %a "$E_HOME")" = 700
      test -w "$E_HOME"
      test -w "$(dirname "$E_SLACK_STATE")"
      test "$(e --version)" = "e 0.0.0-stub"
    '
    ;;
  guard)
    [ "$#" -eq 0 ] || usage
    # The trust boundary, plus the repository's own tooling tests.
    ./scripts/guard.sh
    python3 -m unittest discover -s scripts/release -p 'test_*.py'
    python3 -m unittest discover -s scripts/hooks -p 'test_*.py'
    python3 -m unittest discover -s scripts/ci -p 'test_*.py'
    ;;
  ui)
    cargo build --locked
    # First run creates the env under target/ (gitignored, gone with
    # `cargo clean`); PYTHON points at another interpreter instead.
    if [ -z "${PYTHON:-}" ]; then
      PYTHON=target/ui-env/bin/python
      # The marker records that requirements installed successfully; without
      # it an interrupted or failed pip leaves a reusable-looking venv whose
      # interpreter cannot import pyte, and every later run would skip the
      # repair instead of installing again.
      if [ ! -x "$PYTHON" ] || [ ! -f target/ui-env/.requirements-installed ]; then
        python3 -m venv target/ui-env
        target/ui-env/bin/pip install --quiet -r crates/cli/tests/ui/requirements.txt
        : > target/ui-env/.requirements-installed
      fi
    fi
    "$PYTHON" crates/cli/tests/ui/run.py "$@"
    ;;
  fmt)
    cargo fmt "$@"
    cargo fmt --manifest-path fuzz/Cargo.toml "$@"
    ;;
  lint)
    cargo clippy --workspace --all-targets "$@" -- -D warnings
    # The core and the terminal also build for the browser (crates/web);
    # code behind cfg(target_family = "wasm") is linted there.
    cargo clippy --locked --target wasm32-unknown-unknown -p e-core -p e-tui -- -D warnings
    ;;
  bench)
    [ "$#" -eq 0 ] || usage
    python3 benchmarks/run.py --build --check
    ;;
  audit)
    [ "$#" -eq 0 ] || usage
    # RustSec advisories against Cargo.lock.
    cargo install cargo-audit --version 0.22.2 --locked
    cargo audit
    ;;
  sbom)
    [ "$#" -eq 1 ] || usage
    cargo install cargo-cyclonedx --version 0.5.9 --locked
    python3 scripts/release/sbom.py "$1"
    ;;
  release-check)
    ./scripts/release-check.sh "$@"
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage
    ;;
esac
