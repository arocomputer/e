# ulo for Slack

A Slack bot that runs ulo. Mention it in a channel and it answers in a
thread; every message in that thread continues the same ulo session against
the repository checkout the bot runs in. Tool progress is posted as it
happens, the reply when the turn ends, and when an extension asks a
question the thread gets buttons.

`src/index.ts` handles Slack messages, `rpc.ts` speaks JSONL, and
`threads.ts` owns the connections and saved log paths. Each active thread
has its own `ulo rpc` process, including its extensions. Extension questions
therefore have one owning thread even when several conversations run at
once. Nothing here is compiled into ulo.

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
2. Install ulo on the machine, sign in to a provider (`ulo auth` or a key in
   `~/.ulo/auth.json`), and clone the repository the bot should work in.
3. Trust the checkout (`ulo trust`) so the repository's own `AGENTS.md`, skills,
   and prompts load: the bot has no terminal to answer the trust panel with.
4. Copy `.env.example` to `.env` and fill it in.

## Run it

`@arocomputer/ulo-slack` versions itself and publishes new versions under `latest`
with ulo's production releases. Dev and beta application releases do not republish the
bot; their historical npm tags remain available. **Bump `version` in
`package.json` in the same pull request that changes the bot**: npm refuses to
republish a version, so an unraised version means the change ships in the
repository and nowhere else. It only needs a checkout to work in (`ULO_CWD`):

```sh
npm install -g @arocomputer/ulo-slack
set -a; . ./.env; set +a
ulo-slack
```

`npx @arocomputer/ulo-slack` does the same without a global install.

## Develop

```sh
npm install
npm run typecheck
npm test
set -a; . ./.env; set +a
npm start
```

## Run it on a server

Image publication is paused during development. When releases resume, the bot
will publish as `ghcr.io/arocomputer/ulo-slack` (`:latest` on
the production channel, `:beta` on the beta one), so the host needs neither Node
nor a checkout of ulo:

```sh
docker volume create ulo-slack-home

# Once: trust the checkout and sign in. ulo's home is the volume, so both stick.
docker run --rm -it --user "$(id -u):$(id -g)" \
  -v ulo-slack-home:/home/ulo -v "$PWD:/work" --entrypoint ulo \
  ghcr.io/arocomputer/ulo-slack:latest trust
docker run --rm -it --user "$(id -u):$(id -g)" \
  -v ulo-slack-home:/home/ulo --entrypoint ulo ghcr.io/arocomputer/ulo-slack:latest   # then /login

docker run -d --restart unless-stopped --name ulo-slack \
  --user "$(id -u):$(id -g)" --env-file .env -e ULO_CWD=/work \
  -v ulo-slack-home:/home/ulo -v "$PWD:/work" ghcr.io/arocomputer/ulo-slack:latest
```

`Dockerfile` builds that same image from a checkout — for a patch of your own,
or an architecture the release does not carry. It takes `--build-arg
ULO_VERSION=0.1.0 --build-arg ULO_CHANNEL=production` to pin the release it carries; the
base is Debian 13 because the released Linux binaries link against glibc 2.39.

`--user` is what keeps the files the agent writes in the checkout owned by you
rather than root. `-v "$PWD:/work"` must be the repository the bot should work
in. Instead of signing in, a provider key in the environment works
(`ANTHROPIC_API_KEY`, and the other names in the registry's `key_env` fields).

## What it does

- `@ulo what does this repo do?` in a channel opens a thread and a session
  (`session.create` with `save: true`, named after the thread).
- Messages in the thread are prompts on that session. While a turn runs,
  each finished tool posts one line ("Read `src/main.rs`"); the reply is
  posted when the turn ends, with the cost when the model has pricing.
- `stop` in the thread interrupts the running turn.
- An extension's `ui.confirm` or `ui.select` becomes a message with buttons;
  `ui.input` and `ui.editor` are answered by the next message in the thread.
- The thread's log path is saved to `ULO_SLACK_STATE`. After a restart, its
  next message starts a new process and resumes that path. Old state files
  still load; their process-local session IDs are discarded. State updates replace
  the file atomically. An unreadable or damaged state file stops startup with an
  error and stays untouched; repair it or move it aside to start fresh.
- Shutdown gives each RPC process a deadline, then terminates children that stop
  answering. Pending requests fail when their process exits.
- Question buttons retain the owning connection, so two processes can use
  the same ask number without sending an answer to the wrong conversation.

Everything the bot posts comes from `ulo rpc` events; it never parses
terminal output and never reads `~/.ulo` itself.
