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
3. Trust the checkout (`e trust`) so the repository's own `AGENTS.md`, skills,
   and prompts load: the bot has no terminal to answer the trust panel with.
4. Copy `.env.example` to `.env` and fill it in, including
   `E_SLACK_ALLOWED_USERS` and `E_SLACK_ALLOWED_CHANNELS`: comma-separated Slack
   user IDs and channel IDs. Both must match for a mention, thread reply, stop,
   or approval button to reach e. Missing, empty, or malformed lists stop startup;
   wildcard access is not supported. Only authorized people can answer questions.

Allowlisted users have the agent's filesystem and shell privileges, including
access to provider credentials. Use a dedicated checkout and container or VM
when work needs containment. The agent's child environment excludes `SLACK_*`
credentials, but this is not a sandbox: keep the adapter's `.env` outside its
checkout and other paths the agent can read when separating those credentials.

## Run it

The bot is not published yet. Run it from this directory; it only needs a
checkout to work in (`E_CWD`):

```sh
npm install
set -a; . ./.env; set +a
npm start
```

When releases resume, the bot publishes as `@arocomputer/e-slack` alongside
e's production releases. **Bump `version` in `package.json` in the same pull
request that changes the bot**: npm refuses to republish a version, so an
unraised version means the change ships in the repository and nowhere else.

## Develop

```sh
npm install
npm run typecheck
npm test
set -a; . ./.env; set +a
npm start
```

## Run it on a server

Build the image from the repository root; the host then needs neither Node nor a
checkout of e. With no release published, the build compiles e from source, which
takes a few minutes:

```sh
docker build -t e-slack --build-arg E_UID="$(id -u)" \
  --build-arg E_GID="$(id -g)" services/slack
docker volume create e-slack-home

# Once: trust the checkout and sign in. e's home is the volume, so both stick.
docker run --rm -it \
  -v e-slack-home:/home/e -v "$PWD:/work" --entrypoint e e-slack trust
docker run --rm -it \
  -v e-slack-home:/home/e --entrypoint e e-slack   # then /login

docker run -d --restart unless-stopped --name e-slack \
  --env-file /path/outside/checkout/e-slack.env -e E_CWD=/work \
  -v e-slack-home:/home/e -v "$PWD:/work" e-slack
```

Once releases exist, `--build-arg E_VERSION=0.1.0` installs that release instead
of compiling, and the release workflow publishes the image as
`ghcr.io/arocomputer/e-slack`. Debian 13 supplies a maintained runtime above the
released Linux binaries' glibc 2.31 floor.

The default user is non-root (UID/GID 1000); the build arguments match it to your
checkout's owner. `/home/e` has mode `0700`, and the container stores its Slack
state there by default. A runtime `--user UID:GID` override also works when that
UID owns the mounted home. Existing volumes may need their owner changed to the
selected UID/GID and their directory mode changed to `0700` before restarting.
`-v "$PWD:/work"` must be the repository the bot should work
in. Instead of signing in, a provider key in the environment works
(`ANTHROPIC_API_KEY`, and the other names in the registry's `key_env` fields).

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
  still load; their process-local session IDs are discarded. State updates replace
  the file atomically. An unreadable or damaged state file stops startup with an
  error and stays untouched; repair it or move it aside to start fresh.
- Shutdown gives each RPC process a deadline, then terminates children that stop
  answering. Pending requests fail when their process exits.
- An exited process is evicted; the next message resumes its saved conversation
  in a new process. Idle processes close after `E_SLACK_IDLE_MS` (default 900000,
  fifteen minutes). A running turn is kept alive, and a second turn in the same
  thread is refused until it completes or is stopped.
- `E_SLACK_MAX_THREADS` caps live and opening processes (default 16). At capacity,
  new threads get an error until a process exits or closes. Both limits must be
  positive integers; `E_SLACK_IDLE_MS` must not exceed 2147483647 (Node's timer
  limit). Retiring a process also discards its outstanding questions.
- Question buttons retain the owning connection, so two processes can use
  the same ask number without sending an answer to the wrong conversation.

Everything the bot posts comes from `e rpc` events; it never parses
terminal output and never reads `~/.e` itself.
