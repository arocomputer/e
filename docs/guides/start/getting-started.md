---
title: Getting started
description: install e, connect a model, and run your first task
order: 1
---

# Getting started

e is the coding agent you can put anywhere. It runs on macOS and glibc Linux,
on ARM64 and x86-64, and works in any directory you trust.

## Install

```sh
curl -fsSL https://e.intuitum.sh/install.sh | sh
```

The installer verifies the release for your platform and writes the binary to
`~/.local/bin`. Package managers, preview channels, and builds from source are
on [Install](install.md). `e --version` prints the build you have.

## Open a project

Start e in the directory you want it to work in:

```sh
cd your-project
e
```

The first visit asks whether the directory is trusted. Trust lets e load the
repository's own instructions and resources; it is not a sandbox.

> [!WARNING]
> Tools run with your user's permissions, with no permission prompt by
> default. Use a container, VM, or OS sandbox when the work needs
> containment — [sandboxing](../usage/sandboxing.md) covers the options.

A session with no terminal cannot answer the trust panel; record the decision
first with `e trust [dir]`. [Instructions](../customize/instructions.md)
explains what trust loads.

## Connect a model

Run `/login` and follow the provider's sign-in or API-key flow, then open
`/models` and pick a model. `/login <provider>` connects one by name, and a
provider's usual environment variable (`ANTHROPIC_API_KEY` and friends) works
for scripts.

[Models & providers](../customize/models.md) covers local servers,
`~/.e/models.json`, context windows, and pricing.

## Run your first task

Type a question or describe a change and press enter. e reads files, edits
code, and runs shell commands to answer:

```
Find the authentication entry point.
Explain how a request reaches the session check.
```

For an edit, name the behavior you want and the checks that should pass, then
read the diff and the test output before you commit.

`ctrl+o` opens the transcript reader, `esc` returns to the composer, and
`ctrl+c` cancels the running turn.

## Where to go next

- [Sessions](../usage/sessions.md) — resume, branch, compact, and export a conversation.
- [Settings](../customize/settings.md) — `~/.e`, and every preference in it.
- [Command line](../usage/commands.md) — run options, and the commands inside a session.
- [Extensions](../extend/extensions.md) — add tools, commands, and hooks in any language.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| `e: command not found` | Add `~/.local/bin` to `PATH` and open a new terminal. Package managers install into a directory `PATH` already has. |
| `/models` lists nothing | Run `/login` to connect a provider. For a local model, start its server before opening the picker. |
| The trust panel keeps returning | The directory is untrusted. Answer the panel, or record it with `e trust [dir]`. |
| You need the full reference | `e docs` lists every topic, and `e docs <topic>` prints one. |
