#!/bin/sh
# One repository entry point. Keep CI, contributor docs, and local checks on
# the same commands so no environment has a private definition of "green".
set -eu
cd "$(dirname "$0")"

usage() {
  echo "usage: ./x [check|test|ui|fmt|lint|guard|bench|release-check] [args...]" >&2
  exit 2
}

command=${1:-check}
if [ "$#" -gt 0 ]; then
  shift
fi

case "$command" in
  check)
    [ "$#" -eq 0 ] || usage
    cargo fmt --check
    cargo fmt --manifest-path fuzz/Cargo.toml --check
    cargo clippy --all-targets -- -D warnings
    cargo test --locked
    ./scripts/guard.sh
    ;;
  test)
    cargo test --locked "$@"
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
    cargo clippy --all-targets "$@" -- -D warnings
    ;;
  guard)
    [ "$#" -eq 0 ] || usage
    ./scripts/guard.sh
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
