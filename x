#!/bin/sh
# One repository entry point. Keep CI, contributor docs, and local checks on
# the same commands so no environment has a private definition of "green".
set -eu
cd "$(dirname "$0")"

usage() {
  echo "usage: ./x [dev|scenario|preview|hooks|check|fmt|lint|test|crates|docs|guard|packages|channels|container|ui|bench|release-check] [args...]" >&2
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
  # Each step below is a guarantee, and CI runs them as separate jobs so a
  # failure names itself instead of hiding behind "check". `./x check` is the
  # same steps in the same order, for a contributor's one command.
  check)
    [ "$#" -eq 0 ] || usage
    ./x fmt --check
    ./x lint
    ./x test
    ./x crates
    ./x docs
    ./x guard
    ;;
  test)
    # Every failing suite in one run: without this, the first of 46 test
    # binaries stops the rest and a red pull request reports one problem.
    cargo test --workspace --locked --no-fail-fast "$@"
    ;;
  crates)
    [ "$#" -eq 0 ] || usage
    # The published crates: the application is packaged and built end to end,
    # and the SDK is compiled by an external consumer from its packaged files.
    cargo publish --dry-run --locked --allow-dirty -p intuitums-e
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
    (cd channels/slack && npm ci --no-fund --no-audit && npm run typecheck && npm test)
    python3 -m unittest discover -s channels/github -p 'test_*.py'
    ;;
  container)
    [ "$#" -eq 0 ] || usage
    docker build --tag e-slack channels/slack
    ;;
  guard)
    [ "$#" -eq 0 ] || usage
    # The trust boundary, plus the repository's own tooling tests.
    ./scripts/guard.sh
    python3 -m unittest discover -s scripts/release -p 'test_*.py'
    python3 -m unittest discover -s scripts/hooks -p 'test_*.py'
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
        target/ui-env/bin/pip install --quiet -r tests/ui/requirements.txt
        : > target/ui-env/.requirements-installed
      fi
    fi
    "$PYTHON" tests/ui/run.py "$@"
    ;;
  fmt)
    cargo fmt "$@"
    cargo fmt --manifest-path fuzz/Cargo.toml "$@"
    ;;
  lint)
    cargo clippy --workspace --all-targets "$@" -- -D warnings
    ;;
  bench)
    [ "$#" -eq 0 ] || usage
    python3 benchmarks/run.py --build --check
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
