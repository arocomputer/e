# Services

Applications deployed alongside e live here. The Rust application and SDK
live in `crates/` and do not depend on these services.

| Directory | Purpose | Run or deploy |
| --- | --- | --- |
| [slack](slack/) | Slack bot, one e session per thread | Node.js or Docker |
| [github](github/) | Respond to `/e` on issues and pull requests | Copy the Actions workflow |

The Slack and GitHub services spawn `e rpc` and speak its JSONL protocol.
They are reference clients, not libraries. See the
[channel guide](../docs/guides/usage/channels.md) for their runtime contract.
Run `./x channels` from the repository root to check both clients. The website
lives in [arocomputer/web](https://github.com/arocomputer/web).

Public package distribution is paused during development. When releases resume,
the Slack runner's package name is `@arocomputer/e-slack`.
