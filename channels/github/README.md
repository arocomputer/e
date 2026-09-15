# e on GitHub

`e.yml` is a GitHub Actions workflow: comment `/e <prompt>` on an issue or
pull request and e answers in a comment. Each comment is one headless turn
(`e -p --json`) with the checkout as the working directory, so the
repository's `AGENTS.md` and files are in play exactly as in the terminal.

Install: copy `e.yml` to `.github/workflows/e.yml`, add your provider key
as a repository secret (the workflow reads `ANTHROPIC_API_KEY`; e picks up
any provider's key from its usual environment variable), and comment.

One turn per comment is the right shape for CI: nothing long-lived, the
provider key never leaves the job, and the job's log has the whole event
stream if a reply needs explaining. A repository that wants the
conversation to continue across comments can run `e rpc` in the job with
`save: true` and cache `~/.e/sessions` between runs (`docs/automation.md`).
