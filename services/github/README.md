# ulo on GitHub

`ulo.yml` is a GitHub Actions workflow: comment `/ulo <prompt>` on an issue or
pull request and ulo answers in a comment. Each comment is one headless turn
(`ulo -p --json`) with the checkout as the working directory, so the
repository's `AGENTS.md` and files are in play exactly as in the terminal.

Install: copy `ulo.yml` to `.github/workflows/ulo.yml`, add your provider key
as a repository secret (the workflow reads `ANTHROPIC_API_KEY`; ulo picks up
any provider's key from its usual environment variable), and comment. The
commenter must have repository write, maintain, or admin permission. The
workflow checks that permission before checkout or running ulo; a failed
lookup refuses the run. Checkout does not persist GitHub credentials.

Replies use the issue or pull request number from the event, including
inline review comments. For fork PRs, use a conversation comment: GitHub
withholds provider secrets from fork-triggered review-comment workflows.

One turn per comment is the right shape for CI: nothing long-lived, the
provider key is available to the agent's tools inside that job. Authorized
callers must trust the code they ask the agent to execute. The event stream
is written to `events.jsonl`; retain it as an artifact if needed. A repository that wants the
conversation to continue across comments can run `ulo rpc` in the job with
`save: true` and cache `~/.ulo/sessions` between runs (`docs/guides/usage/automation.md`).
