---
title: Command line
description: Commands, run options, and the commands inside a session.
order: 5
---

# Command line

This is the reference for the `e` command, its run options, and the slash
commands inside a terminal session. `e help` (or `e --help`) prints the same
list, plus any flags, commands, and shortcuts your extensions add.

## Start a session

| Command | What it does |
| --- | --- |
| `e [message]` | start a session in this directory, optionally with a first prompt |
| `e -c`, `e --continue` | continue this directory's most recent session |
| `e -r`, `e --resume` | pick a saved session to resume |
| `e -p [message]` | run one turn headless and print the reply |
| `e rpc` | run the headless session server over stdin and stdout |

A plain `e` does not read piped stdin. Use `e -p` for one turn, or `e rpc` for
a client of your own. See [automation](automation.md) for both.

`-c` and `-r` cannot be combined, and neither combines with `-p`, which always
runs a fresh turn.

## Run options

| Option | What it does |
| --- | --- |
| `-p`, `--print` | run one headless turn. The prompt is the argument, or piped stdin. |
| `-j`, `--json` | machine output for `-p`, `doctor`, and `providers`. With `-p`, one JSON line per event, then a result line. `e --version --json` (long form only) prints the version, channel, and commit. |
| `-m`, `--model <model>` | use this model for the process |
| `--ef`, `--effort <level>` | set reasoning effort for the process |
| `-i`, `--image <path>` | attach an image to the first prompt; repeatable |
| `-P`, `--package <source>` | load a package for this run only; repeatable |
| `--ne`, `--no-extensions` | start without extensions |
| `--nt`, `--no-tools` | expose and run no tools |
| `--ns`, `--no-save` | keep the conversation in memory only |

A value can follow as the next word or after `=` (`--model=openai/gpt-5.5`).
An unknown option is an error, with a suggestion when one is close. To send
prompt text that looks like an option, put it after `--`:
`e -- --help me read this`.

## Commands

| Command | What it does |
| --- | --- |
| `e docs [topic]` | print a built-in guide; without a topic, list them |
| `e install [source]` | install a package, or make every listed one current |
| `e remove <source>` | forget a package and delete its clone |
| `e packages` | list installed packages |
| `e packages init <dir>` | start a package to publish |
| `e trust [dir]` | trust a workspace so e runs there and loads its `AGENTS.md`, skills, and prompts |
| `e untrust [dir]` | refuse that workspace |
| `e auth` | show sign-in status. Sign in with `/login` inside a session. |
| `e providers` | list provider support and sign-in state |
| `e doctor` | print local diagnostics that are safe to paste. `--no-network` is accepted and changes nothing. |
| `e update` | update a release build in place |
| `e help`, `e -h`, `e --help` | print the help |
| `e -v`, `e --version` | print the version |

`e update` only replaces release builds. A build from source, such as one
made with `cargo install`, reports that it is not a release build; update it
by rerunning the install. See [install](../start/install.md).

[Packages](../extend/packages.md) has its own guide. For trust, see
[instructions](../customize/instructions.md).

## Inside a session

| Command | What it does |
| --- | --- |
| `/login [provider]` | sign in to a provider with an account or API key |
| `/models [query]` | switch the model; with a query, pick the first match. `/model` works too. |
| `/effort [level]` | show or set reasoning effort |
| `/scoped-models` | choose which models `ctrl+p` cycles |
| `/reload` | reload extensions, themes, and config |
| `/resume` | resume a saved session |
| `/tree` | rewind to an earlier message and branch in place |
| `/new` | start a fresh session. `/clear` works too. |
| `/fork [name]` | continue in a new session file seeded with this one |
| `/export [path]` | write the active branch as one self-contained HTML page |
| `/copy` | copy the last reply |
| `/compact [focus]` | summarize older context into a fresh session file; a focus steers what it keeps |
| `/usage [period]` | tokens and estimated cost by model; `24h`, `7d` (default), `30d`, or `all` |
| `/undo` | put back what the last write or edit replaced |
| `/trust` | trust this directory |
| `/settings` | change preferences |
| `/help` | show these commands |
| `/version` | show the version |
| `/quit` | exit. `/exit` works too. |
| `!<command>` | run a shell command between turns; the model sees the output |

[Sessions](sessions.md) explains resuming, branching, compaction, and export.

The list grows with what you install. Each prompt template becomes a `/name`
command of its own (see [prompt templates](../customize/prompt-templates.md)),
and extensions add commands the same way.
