---
title: Channels
description: Put e in Slack, GitHub, or Linear as a client of e rpc.
order: 3
---

# Channels

A channel puts e where a team already works, such as a Slack thread, a
Linear issue, or a GitHub pull request. Read this guide to run the reference
channels or to write one for another platform.

A channel is a small program of its own. It runs `e rpc` (or `e -p` for one
turn), maps the platform's conversations to e sessions, and relays events
back. Nothing channel-specific is compiled into e, so Slack tokens and bot
frameworks stay out of the binary, and a channel can be written in any
language. Channels speak the protocol in [automation](automation.md).

The reference channels live under [`services/`](../../../services/):

| Directory | Channel |
| --- | --- |
| [`services/slack/`](../../../services/slack/) | a Slack bot: one thread, one session (TypeScript) |
| [`services/github/`](../../../services/github/) | a GitHub Actions workflow that answers `/e` comments on issues and pull requests |

## How a channel works

Every channel does the same five things.

1. **Spawn `e rpc`.** Keep the pipes open and send `hello`. `ask` lines do
   not carry a session ID, so a channel that relays extension questions runs
   one process per conversation. A client without interactive extensions can
   share one process across sessions. Pass `--no-save` or `--no-tools` if
   the deployment wants them; those bounds hold for every session.
2. **Map a conversation to a session.** A Slack thread, a Linear issue, or a
   pull request is one `session.create`. Set `cwd` to the repository's
   checkout, `save: true` so the conversation survives a restart, and a
   `name` the team recognizes. Keep session IDs in memory, and store the
   mapping from thread to log `path`. After a restart, `session.create` with
   `resume` brings the thread back with its history and a new session ID.
3. **Turn a message into a prompt.** Send `session.prompt` with the text.
   While the turn runs, show `tool_batch` and `tool_end` events in the
   thread, such as "Reading src/main.rs". Accumulate the `text` deltas, or
   use the response's `final_output`, which also carries usage and cost.
4. **Relay questions.** With `hello {ask: true}`, an extension's
   `ui.confirm` or `ui.select` arrives as an `ask` line. Post it as a
   message with buttons, and answer with `ask.reply` when someone clicks.
   Until then the tool waits.
5. **Stop.** Send `session.interrupt` when someone cancels, and
   `session.close` when a thread ends. When the channel itself stops, close
   stdin or send `shutdown`.

A channel never parses terminal output and never reads `~/.e` itself.
Everything it needs is a method: models, sessions, history, and an HTML
export to attach.

### Trust the checkout first

A channel has no terminal, so nothing can answer the trust panel, and `e rpc`
and `e -p` refuse an untrusted directory. Run `e trust <checkout>` once, as
the user and with the home the channel will use. See
[instructions](../customize/instructions.md).

## Slack

`services/slack/` is a Bolt app in socket mode, so it needs no public URL.

The bot answers when mentioned in a channel and continues in the thread.
Each thread owns one `e rpc` process and one saved session against the
checkout in `E_CWD`. The bot posts a line per finished tool, then the reply
when the turn ends. `stop` in the thread interrupts the turn. Extension
questions become a message with buttons.

To run it from a checkout of this repository (Node 22.6 or later):

1. Install e and sign in to a provider. See [install](../start/install.md).
2. Create the Slack app from `services/slack/manifest.json` and collect its
   three credentials, as the channel's
   [README](../../../services/slack/README.md) describes.
3. Trust the repository the bot works in: `e trust /path/to/checkout`.
4. Copy `services/slack/.env.example` to `.env` and fill it in.
5. Start the bot:

   ```sh
   cd services/slack
   npm install
   set -a; . ./.env; set +a
   npm start
   ```

`.env` sets `SLACK_BOT_TOKEN`, `SLACK_SIGNING_SECRET`, `SLACK_APP_TOKEN`, and
`E_CWD`. Optional: `E_BIN` (the `e` binary, default `e` on `PATH`),
`E_MODEL`, and `E_SLACK_STATE` (where the thread-to-session map is kept,
default `./e-slack-state.json`).

`services/slack/Dockerfile` packages the bot and e in one image. With no
release published it compiles e from source; the
[Slack README](../../../services/slack/README.md#run-it-on-a-server) has the
commands.

## GitHub

[`services/github/e.yml`](../../../services/github/e.yml) is a workflow that
runs on issue and pull-request comments that start with `/e `.

The workflow first checks that the commenter has write, maintain, or admin
permission on the repository. Only then does it check out the repository,
build and install e from source (a few minutes, until releases exist), trust
the checkout, and run `e -p --json --no-save` with the rest of the comment as
the prompt. It posts the reply, and the cost when known, as a comment.

To use it, copy it to `.github/workflows/e.yml` in your repository and add a
provider key as a repository secret (the workflow reads `ANTHROPIC_API_KEY`).

One turn per comment fits CI: nothing is long-lived, the checkout is the
working directory, and the provider key is a repository secret. For a
conversation that continues across comments, run `e rpc` with `save: true`
and cache `~/.e/sessions` between runs.

## Linear and other platforms

A Linear channel is the Slack channel with a webhook instead of a socket: an
issue is a session, a comment is a prompt, and the reply is a comment.

Nothing in e distinguishes the platforms; the difference is entirely in the
adapter. Use `services/slack/` as the pattern when you write one.
