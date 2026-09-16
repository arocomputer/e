#!/bin/sh
# One repository entry point. Keep CI, contributor docs, and local checks on
# the same commands so no environment has a private definition of "green".
set -eu
cd "$(dirname "$0")"

usage() {
  echo "usage: ./x [dev|scenario|preview|install-dev|hooks|check|test|ui|fmt|lint|guard|bench|packages|channels|scripts|release-check] [args...]" >&2
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
    project=$(CDPATH='' cd "$project" && pwd)
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
  install-dev)
    exec python3 scripts/release/dev_install.py "$@"
    ;;
  hooks)
    [ "$#" -eq 0 ] || usage
    exec python3 scripts/hooks/install.py
    ;;
  check)
    [ "$#" -eq 0 ] || usage
    cargo fmt --check
    cargo fmt --manifest-path fuzz/Cargo.toml --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --locked
    # Verify package contents and compile the SDK from an external consumer.
    cargo publish --dry-run --locked --allow-dirty -p intuitums-e
    python3 scripts/check-sdk.py
    ./scripts/guard.sh
    python3 -m unittest discover -s scripts/release -p 'test_*.py'
    python3 -m unittest discover -s scripts/hooks -p 'test_*.py'
    ;;
  test)
    cargo test --workspace --locked "$@"
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
  guard)
    [ "$#" -eq 0 ] || usage
    ./scripts/guard.sh
    ;;
  bench)
    [ "$#" -eq 0 ] || usage
    python3 benchmarks/run.py --build --check
    ;;
  packages)
    [ "$#" -eq 0 ] || usage
    python3 -m unittest discover -s scripts/packaging -p 'test_*.py'
    node --test scripts/packaging/publish-npm.test.mjs
    scripts/packaging/smoke.sh
    ;;
  channels)
    [ "$#" -eq 0 ] || usage
    npm --prefix channels/slack ci
    npm --prefix channels/slack run typecheck
    npm --prefix channels/slack test
    python3 -m unittest discover -s channels/github -p 'test_*.py'
    ;;
  scripts)
    [ "$#" -eq 0 ] || usage
    python3 -m unittest discover -s scripts/release -p 'test_*.py'
    python3 -m unittest discover -s scripts/hooks -p 'test_*.py'
    uvx ruff==0.11.13 check --select F,E9 scripts channels/github tests/ui benchmarks
    # actionlint 1.7.12 predates GitHub's concurrency.queue field. Ignore only
    # that schema diagnostic until actionlint supports it; lint the whole file.
    shellcheck --severity=warning x install.sh scripts/*.sh scripts/packaging/*.sh
    actionlint -ignore 'unexpected key "queue" for "concurrency"'
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
