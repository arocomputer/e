---
title: Getting started
description: Install e, connect a model, and run your first task.
order: 1
---

# Getting started

e is the coding agent you can put anywhere. It runs on macOS and glibc Linux,
on ARM64 and x86-64, and works in any directory you trust.

## Install

```sh
curl -fsSL https://e.aro.computer/install.sh | sh
```

The installer verifies the release for your platform and writes the binary to
`~/.local/bin`. Run `e --version` to print the build you have.
[Install](install.md) covers package managers, preview channels, and builds
from source.

## Open a project

Start e in the directory you want it to work in:

```sh
cd your-project
e
```

On the first visit, e asks whether you trust the directory. Trust lets e load
the repository's own instructions and resources. Trust is not a sandbox.

> [!WARNING]
> Tools run with your user's permissions, and by default e shows no permission
> prompt. Use a container, VM, or OS sandbox when the work needs containment.
> [Sandboxing](../usage/sandboxing.md) covers the options.

A session with no terminal cannot answer the trust panel. Record the decision
first with `e trust [dir]`. [Instructions](../customize/instructions.md)
explains what trust loads.

## Connect a model

Run `/login` and follow the provider's sign-in or API-key flow. Then open
`/models` and pick a model.

- `/login <provider>` connects one provider by name.
- For scripts, a provider's usual environment variable works, such as
  `ANTHROPIC_API_KEY`.

[Models & providers](../customize/models.md) covers local servers,
`~/.e/models.json`, context windows, and pricing.

## Run your first task

Type a question or describe a change, then press enter. e reads files, edits
code, and runs shell commands to answer:

```
Find the authentication entry point.
Explain how a request reaches the session check.
```

For an edit, name the behavior you want and the checks that should pass. Read
the diff and the test output before you commit.

| Key | Action |
| --- | --- |
| `ctrl+o` | open the transcript reader |
| `esc` | return to the composer |
| `ctrl+c` | cancel the running turn |

## Where to go next

- [Sessions](../usage/sessions.md): resume, branch, compact, and export a conversation.
- [Settings](../customize/settings.md): `~/.e`, and every preference in it.
- [Command line](../usage/commands.md): run options, and the commands inside a session.
- [Extensions](../extend/extensions.md): add tools, commands, and hooks in any language.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| `e: command not found` | Add `~/.local/bin` to `PATH` and open a new terminal. Package managers install into a directory `PATH` already has. |
| `/models` lists nothing | Run `/login` to connect a provider. For a local model, start its server before you open the picker. |
| The trust panel keeps returning | The directory is untrusted. Answer the panel, or record the decision with `e trust [dir]`. |
| You need the full reference | `e docs` lists every topic, and `e docs <topic>` prints one. |
