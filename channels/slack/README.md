# e for Slack

A Slack bot that runs e. Mention it in a channel and it answers in a
thread; every message in that thread continues the same e session against
the repository checkout the bot runs in. Tool progress is posted as it
happens, the reply when the turn ends, and when an extension asks a
question the thread gets buttons.

`src/index.ts` handles Slack messages, `rpc.ts` speaks JSONL, and
`threads.ts` owns the connections and saved log paths. Each active thread
has its own `e rpc` process, including its extensions. Extension questions
therefore have one owning thread even when several conversations run at
once. Nothing here is compiled into e.

## Setup

1. Create the app from `manifest.json` (api.slack.com/apps → *Create New App*
   → *From an app manifest*). It turns on socket mode and interactivity and
   carries the bot token scopes `app_mentions:read`, `channels:history`,
   `groups:history`, `chat:write`, and `reactions:read`, and the bot events
   `app_mention`, `message.channels`, and `message.groups`. Upload
   `../../assets/icon.svg` and `../../assets/logo.svg` as the app icon, then
   generate an app-level token with `connections:write` (*Basic Information* →
   *App-Level Tokens*) — the manifest cannot mint one for you. Interactivity
   and socket mode need no request URL.
2. Install e on the machine, sign in to a provider (`e auth` or a key in
   `~/.e/auth.json`), and clone the repository the bot should work in.
3. Copy `.env.example` to `.env` and fill it in.

## Run it

`@intuitums/e-slack` ships with e's releases; its version matches the release
it came from. It only needs a checkout to work in (`E_CWD`):

```sh
npm install -g @intuitums/e-slack
set -a; . ./.env; set +a
e-slack
```

`npx @intuitums/e-slack` does the same without a global install.

## Develop

```sh
npm install
npm run typecheck
npm test
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
- The thread's log path is saved to `E_SLACK_STATE`. After a restart, its
  next message starts a new process and resumes that path. Old state files
  still load; their process-local session IDs are discarded.
- Question buttons retain the owning connection, so two processes can use
  the same ask number without sending an answer to the wrong conversation.

Everything the bot posts comes from `e rpc` events; it never parses
terminal output and never reads `~/.e` itself.
