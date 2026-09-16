#!/bin/sh
# The security-surface audit. Run locally before pushing; CI runs it on every
# PR. Each check pins a promise e makes to its users — a PR that moves one of
# these boundaries must change this script in the same diff, where the review
# can see it.
set -eu
cd "$(dirname "$0")/.."
fail=0

say() { printf '%s\n' "$*"; }
bad() { fail=1; say "FAIL: $*"; }

# Production-only view of a Rust source tree: truncate each file at its
# first #[cfg(test)] marker before scanning. A test module is always the
# last item in a file — CI denies clippy::items_after_test_module — so
# nothing shipped can follow it. Without this, test fixtures (a placeholder
# host like example.invalid, a scratch File::write into a temp dir) trip
# boundaries these checks mean for the shipped binary, not its tests.
prod_rs() {
  for f in "$@"; do
    awk -v file="$f" '
      /^#\[cfg\(test\)\]/ { exit }
      { print file":"FNR":"$0 }
    ' "$f"
  done
}

# 1. Network surface. e talks to its sign-in and model providers, and to
#    models.dev for model facts (decision 0008, an unauthenticated GET that
#    carries no user data), and nothing else — in the shipped binary (crates/*/src)
#    or its dev tooling (scripts/). A new host means a new place user data
#    can go — add it here deliberately or the build fails.
# Numeric loopback is used by crates/cli/tests/ui/run.py's synthetic streaming server.
allowed_hosts="localhost 127.0.0.1 models.dev auth.openai.com api.openai.com chatgpt.com opencode.ai auth.x.ai api.x.ai api.anthropic.com api.github.com www.npmjs.com github.com registry.npmjs.org e.intuitum.sh ai-gateway.vercel.sh generativelanguage.googleapis.com api.groq.com api.mistral.ai api.deepseek.com api.cerebras.ai openrouter.ai api.together.xyz api.fireworks.ai"
found_hosts=$(
  { prod_rs $(find crates/*/src -name '*.rs' 2>/dev/null); find scripts crates/cli/tests/ui -type f ! -path '*/__pycache__/*' -exec cat -- {} + 2>/dev/null; } |
    grep -ohE 'https?://[A-Za-z0-9.-]+' | sed -E 's#https?://##' | sort -u
)
for host in $found_hosts; do
  case " $allowed_hosts " in
    *" $host "*) ;;
    *) bad "unlisted network host in crates/*/src or scripts/: $host (grep it, then extend guard.sh deliberately)" ;;
  esac
done

# 2. Sovereign home. e reads only ~/.e — never another tool's store.
if out=$(grep -rnE '[~/"]\.(claude|codex|cursor|gemini|opencode|aws|ssh)\b' crates/*/src --include='*.rs' 2>/dev/null); then
  bad "reference to another tool's home directory:"
  say "$out"
fi

# 3. Home resolution happens in one place. HOME/E_HOME lookups outside these
#    files are a second door into the filesystem — `var_os` included, so the
#    OsString form cannot slip past the pattern.
if out=$(grep -rn 'env::var("HOME")\|env::var("E_HOME")\|env::var_os("HOME")\|env::var_os("E_HOME")' crates/*/src --include='*.rs' |
    grep -v '^crates/core/src/config/home.rs:' | grep -v '^crates/tui/src/app/mod.rs:' |
    grep -v '^crates/tui/src/app/frame.rs:'); then
  bad "HOME/E_HOME read outside core/config/home.rs (or tui/app's title display):"
  say "$out"
fi

# 4. Config and credential writes go through core/store.rs — the merge-write
#    path that never wipes unknown keys and chmods auth to 0600. Direct write
#    APIs in core are limited to the files that own a format: packages.rs
#    owns ~/.e/packages/ (the npm project file there) and the files
#    `e packages init` scaffolds into a directory the user names; its
#    settings entries still go through the store.
if out=$(prod_rs $(find crates/core/src -name '*.rs' 2>/dev/null) | grep -E 'fs::write|File::create|OpenOptions' |
    grep -v '^crates/core/src/config/store.rs:' | grep -v '^crates/core/src/session.rs:' |
    grep -v '^crates/core/src/config/home.rs:' | grep -v '^crates/core/src/tools/' |
    grep -v '^crates/core/src/update.rs:' | grep -v '^crates/core/src/providers/diagnostics.rs:' |
    grep -v '^crates/core/src/resources/packages.rs:'); then
  bad "direct file write in crates/core outside audited store/session/tool/update/diagnostics paths:"
  say "$out"
fi

# 5. Unsafe code stays where it is audited: the libc terminal poll and the
#    bash tool's process-group kill (setsid + SIGKILL at the timeout).
if out=$(grep -rnE 'unsafe (fn|impl|\{)' crates/*/src --include='*.rs' | grep -v '^crates/tui/src/paint/background.rs:' | grep -v '^crates/core/src/tools/bash.rs:'); then
  bad "unsafe code outside tui/paint/background.rs, core/tools/bash.rs:"
  say "$out"
fi

# 6. Workflow actions are pinned by commit SHA — a moved tag must not be able
#    to rewrite our CI.
for wf in .github/workflows/*.yml; do
  [ -f "$wf" ] || continue
  if out=$(grep -n 'uses:' "$wf" | grep -vE '@[0-9a-f]{40}'); then
    bad "workflow action not pinned to a full commit SHA in $wf:"
    say "$out"
  fi
done

# 7. Exact CODEOWNERS paths must exist. This prevents ownership silently
#    disappearing after a directory rename; glob patterns remain valid and
#    are deliberately skipped here.
for pattern in $(awk '!/^#/ && NF { print $1 }' .github/CODEOWNERS); do
  case "$pattern" in
    '*'|*'*'*|*'?'*|*'['*) continue ;;
  esac
  target=${pattern#/}
  if [ ! -e "$target" ]; then
    bad "CODEOWNERS path does not exist: $pattern"
  fi
done

# 8. The core stays terminal-free and the frontends stay apart. core depends
#    on no frontend crate and no terminal library, so the harness the SDK
#    embeds carries no terminal with it; tui, rpc, and sdk depend on core and
#    never on each other. Cargo enforces the imports; this pins the manifests.

if out=$(grep -nE '^(e-tui|e-rpc|e-sdk|crossterm|unicode-width)\b|intuitums-e-(tui|rpc|sdk)' crates/core/Cargo.toml); then
  bad "crates/core depends on a frontend or a terminal library:"
  say "$out"
fi
for frontend in tui rpc; do
  if out=$(grep -nE '^e-(tui|rpc)\b' "crates/$frontend/Cargo.toml"); then
    bad "crates/$frontend depends on another frontend:"
    say "$out"
  fi
done
if out=$(sed -n '/^\[dependencies\]/,/^\[/p' crates/sdk/Cargo.toml | grep -nE 'intuitums-e"|intuitums-e-(tui|rpc)'); then
  bad "crates/sdk depends on a frontend:"
  say "$out"
fi

# 9. The test workflow uses the local check commands. Raw `cargo` or `npm`
#    calls would create a second definition of passing, one
#    the local check does not have; the specialized workflows (release,
#    security, docs) are their own thing and are not fenced.
if out=$(grep -nE 'run: .*\b(cargo|npm|python3 -m unittest|scripts/packaging)' .github/workflows/checks.yml 2>/dev/null); then
  bad "a check calls a tool directly; call ./x <step> instead
$out"
fi

if [ "$fail" -eq 0 ]; then
  say "guard: all checks passed"
else
  exit 1
fi
