---
title: Instructions
description: Give the agent standing instructions with AGENTS.md files.
order: 3
---

# Instructions

`AGENTS.md` files hold instructions that e adds to the system prompt. e wraps
each one as project instructions, with the file's path.

## Where e looks

| File | Scope | When it loads |
| --- | --- | --- |
| `~/.e/AGENTS.md` | Yours, for every project. | Always. |
| `<workspace>/AGENTS.md` | The project's. | Once the directory is trusted. |
| `<workspace>/<dir>/…/AGENTS.md` | Nested, for one directory. | The first time a tool touches a path under that directory. |

Trust a workspace with `/trust`, or with `e trust [dir]` when there is no
terminal. A run refuses an untrusted workspace outright, because an untrusted
repository could otherwise steer the agent.

Files are capped at 32 KiB each.

## Nested instructions

Nested files keep a monorepo's rules local. `services/api/AGENTS.md` is not in
context until the agent touches `services/api/`. Then it loads, without anyone
asking.

- A nested file loads the first time a tool reads, writes, edits, or searches
  a path under its directory.
- It arrives as a message in the conversation, not in the system prompt.
- The nearest file arrives last, so it reads as the most specific.
- Each nested file loads once per session.
- Nested files load only in a trusted workspace, and only for paths inside it.

## Trust is the precondition

e runs only in a directory whose own instructions you have accepted. A
session that started untrusted would work in a repository with no say in what
the model was told.

- On your first visit, e asks with the terminal's trust panel.
- Declining exits and records nothing, so the next launch asks again.
- `e trust [dir]` records the answer for a session with no terminal.
- `e untrust [dir]` refuses that directory deliberately.

Trust extends to everything inside a trusted ancestor. For that reason e never
records your home directory as trusted.

## Related guides

Reasoning, skills, and prompt templates have their own guides:
`e docs skills` and `e docs prompt-templates`.
