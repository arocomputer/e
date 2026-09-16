# Channels

Reference programs that put e in a team's tools by spawning `e rpc` and
speaking its JSONL protocol (`docs/guides/usage/automation.md`). Each is a consumer of
the binary, like `crates/sdk/` is a consumer of the core: nothing here is
compiled into e, and nothing here is required to run it. The pattern they
share is described in `docs/guides/usage/channels.md`.

```
slack/     a Slack bot: one process and session per thread (TypeScript, Bolt)
github/    a GitHub Actions workflow answering `/e` on issues and PRs
```

These are starting points to copy, not libraries to depend on. A company's
own channel will differ in where it keeps the thread-to-session map and
what it posts; the protocol underneath is the supported contract.

The Slack bot is also published as `@intuitums/e-slack`, so it runs on a
machine that has no checkout of this repository (`npx @intuitums/e-slack`).
That changes how it is installed, not what it is: a reference program with
its own version, published under `latest` with stable releases. The package is a runner — its
JavaScript surface is not a supported API. Anything that needs a contract
uses the protocol.
