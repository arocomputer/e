# e for Slack

A Slack bot that runs e. Mention it in a channel and it answers in a
thread; every message in that thread continues the same e session against
the repository checkout the bot runs in. Tool progress is posted as it
happens, the reply when the turn ends, and when an extension asks a
question the thread gets buttons.

It is one file, `src/index.ts`, and it depends on two things: Slack's Bolt
framework and a spawned `e rpc` (`docs/automation.md`). Nothing here is
compiled into e. Copy it and change what your team wants posted.

## Setup

1. Create a Slack app with socket mode on. Bot token scopes:
   `app_mentions:read`, `channels:history`, `groups:history`, `chat:write`,
   `reactions:read`. Subscribe to the bot events `app_mention` and
   `message.channels` (and `message.groups` for private channels). Enable
   interactivity (socket mode needs no request URL).
2. Install e on the machine, sign in to a provider (`e auth` or a key in
   `~/.e/auth.json`), and clone the repository the bot should work in.
3. Copy `.env.example` to `.env` and fill it in.

```sh
npm install
npm run typecheck
set -a; . ./.env; set +a
npm start
```

## What it does

- `@e what does this repo do?` in a channel opens a thread and a session
  (`session.create` with `save: true`, named after the thread).
- Messages in the thread are prompts on that session. While a turn runs,
  each finished tool posts one line ("Read `src/main.rs`"); the reply is
  posted when the turn ends, with the cost when the model has pricing.
- `stop` in the thread interrupts the running turn.
- An extension's `ui.confirm` or `ui.select` becomes a message with buttons;
  `ui.input` and `ui.editor` are answered by the next message in the thread.
- The thread → session map is saved to `E_SLACK_STATE`, so after a restart
  the next message in an old thread resumes its session from disk.

Everything the bot posts comes from `e rpc` events; it never parses
terminal output and never reads `~/.e` itself.
