---
title: Getting started
description: Install e, connect a model, and run your first task.
order: 1
---

# Getting started

e is a coding agent that runs in your terminal. It reads your code, edits
files, and runs commands with the model you choose. This guide takes you from
install to a first finished task.

## Install

e is not published yet, so you build it from source. You need Rust 1.98 or
newer, from [rustup](https://rustup.rs).

```sh
cargo install --locked --git https://github.com/arocomputer/e e
```

This puts `e` in `~/.cargo/bin`, which rustup adds to your `PATH`. Check it
with `e --version`. [Install](install.md) covers updating, where e keeps its
files, and running a checkout you are editing.

## Open a project

Start e in the directory you want it to work in:

```sh
cd /path/to/project
e
```

The first time, e asks whether you trust the directory. Trusting it lets e
load the project's own instructions and resources;
[Instructions](../customize/instructions.md) lists what that includes. Trust
is not a sandbox.

> [!WARNING]
> Tools run with your user's permissions, and e does not ask before running
> them. Use a container, VM, or OS sandbox when the work needs containment.
> [Sandboxing](../usage/sandboxing.md) covers the options.

Where nobody can answer the trust prompt, as in a script or CI, record the
decision first with `e trust /path/to/project`.

## Connect a model

Inside e, run `/login` and follow your provider's sign-in or API-key flow.
Then pick a model with `/models`.

- `/login <provider>` goes straight to one provider.
- In scripts, set the provider's usual environment variable instead, such as
  `ANTHROPIC_API_KEY`.

[Models & providers](../customize/models.md) covers local models, custom
providers, context windows, and pricing.

## Run your first task

Describe what you want and press enter. e reads files, edits code, and runs
shell commands to do it:

```
Find the authentication entry point and explain how a request reaches the session check.
```

For a change, name the behavior you want and the check that should pass, such
as a test command. Review the diff and the test output before you commit.

| Key | Action |
| --- | --- |
| `ctrl+c` | stop the running turn; press twice quickly to quit |
| `ctrl+o` | open the transcript reader |
| `esc` | close the reader |

## Next steps

- [Sessions](../usage/sessions.md): resume, branch, compact, and export conversations.
- [Command line](../usage/commands.md): flags, subcommands, and the slash commands inside a session.
- [Settings](../customize/settings.md): where e keeps its files, and every preference.
- [Extensions](../extend/extensions.md): add tools, commands, and hooks in any language.

In the terminal, `e docs` lists every guide and `e docs <topic>` prints one.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `e: command not found` | Add `~/.cargo/bin` to `PATH` and open a new terminal. |
| `/models` shows no models | Run `/login` first. For a local model, start its server before you open the picker. |
| The trust prompt keeps coming back | Answer it, or run `e trust` in the project directory. |
