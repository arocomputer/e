# Channels

Reference programs that put e in a team's tools by spawning `e rpc` and
speaking its JSONL protocol (`docs/automation.md`). Each is a consumer of
the binary, like `sdk/` is a consumer of the library: nothing here is
compiled into e, and nothing here is required to run it. The pattern they
share is described in `docs/channels.md`.

```
slack/     a Slack bot — one thread, one e session (TypeScript, Bolt)
github/    a GitHub Actions workflow answering `/e` on issues and PRs
```

These are starting points to copy, not packages to depend on. A company's
own channel will differ in where it keeps the thread-to-session map and
what it posts; the protocol underneath is the supported contract.
