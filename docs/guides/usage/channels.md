---
title: Channels
description: Put e in Slack, GitHub, or Linear as a client of e rpc.
order: 3
---

# Channels

A channel puts e where a team already works, such as a Slack thread, a
Linear issue, or a GitHub pull request.

Each channel is a small program of its own. It spawns `e rpc`, maps the
platform's conversations to e sessions, and relays events back. e does not
ship channels inside the binary. In the same way, the terminal frontend
consumes the core but is not part of it.

This design keeps Slack tokens, webhooks, and bot frameworks out of the
binary every developer installs. It also lets a company write its channel in
whatever language its glue code already uses.

Channels speak the protocol described in [Automation](automation.md). The
reference channels live under [`channels/`](../../../channels/) in the
repository:

```
channels/slack/     a Slack bot: one thread, one session (TypeScript)
channels/github/    a GitHub Actions workflow answering `/e` on issues and PRs
```

## How a channel works

Every channel does the same five things.

1. **Spawn `e rpc`.** Keep the pipes open and say `hello`. `ask` lines do
   not carry a session ID, so a channel that relays extension questions uses
   one process per conversation. A client without interactive extensions can
   share one process across sessions. Pass `--no-save` or `--no-tools` if
   the deployment wants them. Those bounds hold for every session.
2. **Map a conversation to a session.** A Slack thread, a Linear issue, or a
   PR is one `session.create`. Set `cwd` to the repository's checkout,
   `save: true` so the conversation survives a restart, and a `name` the
   team recognizes. Keep session IDs in memory, and save the mapping from
   thread ID to log path. After a restart, `session.create` with `resume`
   brings the thread back with its history and a new session ID.
3. **Turn a message into a prompt.** Send `session.prompt` with the text.
   While the turn runs, show the `tool_batch` and `tool_end` events in the
   thread, such as "Reading src/main.rs" or "Ran tests". Accumulate the
   `text` deltas into the reply. The response line carries the final text,
   usage, and cost.
4. **Relay questions.** With `hello {ask: true}`, an extension's
   `ui.confirm` or `ui.select` arrives as an `ask` line. Post it as a
   message with buttons, and answer with `ask.reply` when someone clicks.
   Until then the tool waits.
5. **Stop.** Send `session.interrupt` for a cancel reaction and
   `session.close` when a thread is archived. When the channel process
   stops, close stdin or send `shutdown`.

A channel never parses terminal output and never touches `~/.e` itself.
Everything it needs is a method, including models, sessions, history, and an
HTML export to attach.

## Slack

`channels/slack/` is the reference channel. It is a Bolt app in socket mode,
so it needs no public URL.

The bot answers when mentioned in a channel and continues in the thread.
Each thread owns one process and one session against the checkout in
`E_CWD`. The bot posts tool progress as it happens and the reply when the
turn ends. Extension questions become a message with buttons.

The channel's README covers the app manifest, the three Slack credentials,
and how to run it.

### Run with npx

The channel publishes with e's releases as `@intuitums/e-slack`. Run
`npx @intuitums/e-slack` against any checkout without cloning this
repository. Its version matches the release it came from.

### Run on a server

`channels/slack/Dockerfile` builds an image that carries e from the release
and the bot from the repository. Each release publishes the image as
`ghcr.io/intuitums/e-slack`, so the usual case needs neither Node nor a
checkout.

To deploy it:

- Mount the checkout at `/work`.
- Put e's home on a volume.
- Record the trust decision once with `e trust`.

The trust step matters because a channel has no terminal. Nothing else can
answer the panel that gates the repository's own instructions.

## GitHub

`channels/github/e.yml` is a workflow that runs on issue and pull-request
comments containing `/e`.

The workflow first verifies that the commenter has write, maintain, or admin
permission on the repository. Only then does it check out the repository,
install e, and run `e -p --json` with the comment as the prompt. It posts
the reply as a comment.

One turn per comment is the right shape for CI. Nothing is long-lived, the
checkout is the working directory, and the provider key is a repository
secret.

If a conversation must continue across comments, use `e rpc` with
`save: true` and cache `~/.e/sessions` between runs.

## Linear and other platforms

A Linear channel is the Slack channel with a webhook instead of a socket. An
issue is a session, a comment is a prompt, and the reply is a comment.

Nothing in e distinguishes the platforms. The difference is entirely in the
adapter. Use `channels/slack/` as the pattern when you write one.
