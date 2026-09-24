# Fuzzing

This independent, unpublished workspace tests boundaries across the core and
terminal libraries. It belongs at the repository root because the targets do
not belong to the CLI executable:

- `sse`: bounded server-sent event parsing.
- `extension_protocol`: JSONL messages from extension processes.
- `sanitize`: terminal text sanitization.

The security workflow runs all three weekly and on manual dispatch, for two
minutes each. Each target has a checked-in starting corpus. Keep fuzzing
separate from ordinary tests because it needs nightly Rust and libFuzzer.

From the repository root, with the pinned nightly and cargo-fuzz installed:

```sh
cargo +nightly-2026-08-20 fuzz run sse -- -max_total_time=120
```

Replace `sse` with another target name as needed. Generated corpus entries,
crash artifacts, and build output stay ignored. A confirmed crash should
become a focused regression test in `crates/cli/tests/`.
