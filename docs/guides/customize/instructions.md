---
title: Instructions
description: Give the agent standing instructions with AGENTS.md files.
order: 3
---

# Instructions

`AGENTS.md` files hold standing instructions that e gives the model in every
session. This guide covers where to put them and how workspace trust decides
which ones load.

## Add instructions

Write plain Markdown to one of these files:

| File | Scope | When it loads |
| --- | --- | --- |
| `~/.e/AGENTS.md` | Every workspace. | Always, in the system prompt. |
| `<workspace>/AGENTS.md` | The workspace e runs in. | In the system prompt, once the workspace is trusted. |
| `<workspace>/<dir>/…/AGENTS.md` | One directory inside the workspace. | As a message, the first time a tool touches a path under that directory. |

e creates an empty `~/.e/AGENTS.md` the first time it writes to its home. An
empty file adds nothing. e wraps each file in a `<project_instructions>`
block that carries the file's path. e rereads the home and workspace files
for each prompt you send, so an edit to them needs no restart.

To replace e's built-in system prompt itself, set `system_prompt` in
[settings](settings.md#agent). `AGENTS.md` files are still appended to it.

## Nested instructions

A nested file keeps a monorepo's rules local: `services/api/AGENTS.md` enters
the conversation only when the agent works in `services/api/`.

- It loads the first time `read`, `write`, `edit`, or `grep` names a path
  under its directory. For `grep`, the searched directory's own file counts.
- It arrives as a message in the conversation, not in the system prompt.
- When several load at once, the nearest one arrives last, so it reads as the
  most specific.
- Each one loads once per session. A resumed or forked session remembers
  which ones it already has.
- Only in a trusted workspace, and only for paths inside it, including after
  symlinks are resolved.
- Only the first 32 KiB is read; a longer file is clipped with a notice.

## Trust

e runs only in a trusted workspace, because a workspace's `AGENTS.md`,
`.e/skills/`, and `.e/prompts/` steer the agent. Trust decides whether they
load.

- On the first visit, the terminal asks. You can trust the directory, trust
  a broader parent that contains it, or decline.
- Declining exits without recording anything, so the next launch asks again.
- `/trust` trusts the current directory from inside a session.
- `e trust [dir]` records trust without a terminal, for scripts, CI, and
  [`e rpc`](../usage/automation.md).
- `e untrust [dir]` records a refusal. e then refuses to run there until you
  trust it again.
- A run with no terminal, such as `e -p`, refuses a workspace that is not
  trusted.

Trusting a directory covers everything inside it. For that reason e refuses
to trust your home directory or any directory that contains it. Decisions are
stored in `~/.e/trust.json`.

## Related guides

- [Skills](skills.md) and [prompt templates](prompt-templates.md), which a
  trusted workspace can also carry.
- [Sandboxing](../usage/sandboxing.md), for what trust does and does not
  protect.
