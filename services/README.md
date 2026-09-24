# Services

Applications deployed alongside ulo live here. The Rust application and SDK
live in `crates/` and do not depend on these services.

| Directory | Purpose | Run or deploy |
| --- | --- | --- |
| [www](www/) | Website and public guides at ulo.sh | Astro and Cloudflare Workers |
| [slack](slack/) | Slack bot, one ulo session per thread | Node.js or Docker |
| [github](github/) | Respond to `/ulo` on issues and pull requests | Copy the Actions workflow |

The Slack and GitHub services spawn `ulo rpc` and speak its JSONL protocol.
They are reference clients, not libraries. See the
[channel guide](../docs/guides/usage/channels.md) for their runtime contract.
Run `./x channels` from the repository root to check both clients. The website
has its own commands in [www/README.md](www/README.md).

Public package distribution is paused during development. When releases resume,
the Slack runner's package name is `@arocomputer/ulo-slack`.
