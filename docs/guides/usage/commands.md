---
title: Command line
description: Commands, run options, and the commands inside a session.
order: 5
---

# Command line

`ulo help` prints this list, with any flags and commands your extensions add.

## Start a session

| Command | What it does |
| --- | --- |
| `ulo [message]` | start a session in this directory, optionally with a first prompt |
| `ulo -c` | continue this directory's most recent session |
| `ulo -r` | pick a saved session to resume |
| `ulo -p [message]` | run one turn headless and print the reply |
| `ulo rpc` | run the headless session server over stdin and stdout |

A plain `ulo` does not read piped stdin. Use `ulo -p` for one turn, or `ulo rpc` for
a client of your own. See [automation](automation.md).

## Run options

| Option | What it does |
| --- | --- |
| `-p`, `--print` | run one headless turn. The prompt is the argument or piped stdin. |
| `-j`, `--json` | machine output for `--print`, `doctor`, `providers`, and `--version`. With `-p`, one JSON line per event and a result line. |
| `-m`, `--model <model>` | use this model for the process |
| `--ef`, `--effort <level>` | set reasoning effort for the process |
| `-i`, `--image <path>` | attach an image to the first prompt, repeatable |
| `-P`, `--package <source>` | load a package for this run only, repeatable |
| `--ne`, `--no-extensions` | start without extensions |
| `--nt`, `--no-tools` | expose and run no tools |
| `--ns`, `--no-save` | keep the conversation in memory only |

## Commands

| Command | What it does |
| --- | --- |
| `ulo docs [topic]` | print a built-in guide |
| `ulo update` | update to the latest release |
| `ulo install [source]` | install a package, or make every listed one current |
| `ulo remove <source>` | forget a package and delete its clone |
| `ulo packages` | list installed packages |
| `ulo packages init <dir>` | start a package to publish |
| `ulo trust [dir]` | trust a workspace's `AGENTS.md`, skills, and prompts |
| `ulo untrust [dir]` | stop loading them for that workspace |
| `ulo auth` | show sign-in status |
| `ulo providers` | list provider support and sign-in state |
| `ulo doctor [--no-network]` | print diagnostics that are safe to paste |
| `ulo help` | print the help |
| `ulo -v`, `ulo --version` | print the version |

[Packages](../extend/packages.md) has its own guide. For `ulo trust`, see
[instructions](../customize/instructions.md).

## Inside a session

| Command | What it does |
| --- | --- |
| `/login` | sign in to a provider with an account or API key |
| `/models` | switch the model |
| `/effort` | show or set reasoning effort |
| `/scoped-models` | choose which models `ctrl+p` cycles |
| `/reload` | reload extensions, themes, and config |
| `/resume` | resume a saved session |
| `/tree` | rewind to an earlier message and branch in place |
| `/new` | start a fresh session |
| `/fork` | continue in a new session file seeded with this one |
| `/export` | write this session as one self-contained HTML page |
| `/copy` | copy the last reply |
| `/compact` | summarize into a fresh session. `/compact <focus>` steers what it keeps. |
| `/usage` | tokens and estimated cost by model. `/usage 24h` narrows the window. |
| `/undo` | put back what the last write or edit replaced |
| `/trust` | trust this directory |
| `/settings` | change preferences |
| `/help` | show these commands |
| `/quit` | exit |

The list follows what you install. Each prompt template becomes a `/name`
command of its own. See [prompt templates](../customize/prompt-templates.md).
An extension adds commands the same way.
