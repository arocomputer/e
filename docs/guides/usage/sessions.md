---
title: Sessions
description: Resume, branch, compact, and export a conversation.
order: 1
---

# Sessions

A session is one conversation and its history: messages, tool calls, their
output, and the usage they recorded. Read this guide to pick up earlier work,
try a different path from an earlier message, or keep a long task within the
model's context.

## Resume a session

| Command | Effect |
| --- | --- |
| `e` | start a new session in this directory |
| `e -c` | continue this directory's most recent session |
| `e -r` | pick a saved session to resume |
| `/resume` | pick a saved session from inside the terminal |
| `/new` | start a fresh session in place of this one |

The picker has two tabs: **Current workspace** and **All workspaces**. A
session shows its name there when it has one. An extension can set the name,
`/fork <name>` names the fork, and `e rpc` takes a `name` in
`session.create`.

## Where sessions live

e saves each session as one JSONL file under `~/.e/sessions/`, in a folder
per working directory, and appends to it as the turn runs. Session files are
created readable by your user only. [Settings](../customize/settings.md)
describes the home directory and what else it holds.

> [!IMPORTANT]
> Treat session files as project data. They hold what the tools read: source,
> configuration, and anything else the model was shown. A shared session or an
> [export](#export) is as sensitive as the repository.

## Branch in place

Every message has an id and a parent, so a session is a tree, not a line.
Branching lets you go back to an earlier message and try a different path.

1. Run `/tree` and pick an earlier message.
2. e rewinds the conversation to that point and puts that message's prompt
   back in the composer.
3. Edit the prompt, or resend it as is, to start the new branch.

The branch you left stays in the file, and `/tree` reaches it the same way.
Nothing is rewritten.

Branching changes the conversation, not the working directory. It does not
undo edits a tool already made. `/undo` puts back what the last `write` or
`edit` replaced, one change at a time, for up to 100 changes (files larger
than 8 MiB are not kept). Use git for anything else.

### Fork a session

`/fork [name]` copies the current branch into a new session file and
continues there. Use it when the next task deserves a separate history rather
than a second branch. The original file is unchanged. Without a name, a named
session's fork is called `<name> (fork)`.

In `e rpc`, `session.fork` does the same, and `session.create` with `resume`
reopens a saved file. See [automation](automation.md).

## Context and compaction

When the model's context window fills, e summarizes the older messages and
continues the same turn. You do not restart the task. The summary and the
most recent messages seed a fresh session file; the previous file stays
resumable.

Run `/compact` to summarize by hand. Add a focus to steer what the summary
keeps, such as decisions, unfinished work, or a subsystem:

```
/compact <focus>
```

Compaction keeps every user instruction and every earlier summary. If those
alone do not fit, compaction fails and the history stays as it was.

## Usage and cost

`/usage` reports tokens and estimated cost by model across all saved
sessions. The default window is the last 7 days; `/usage 24h`, `/usage 30d`,
and `/usage all` change it. The report includes compaction's own requests.

## Export

`/export [path]` writes the active branch as one self-contained HTML page:
the conversation, the tool activity, and the code it touched. Without a path,
e writes `e-session-<id>.html` in the working directory. A relative path is
resolved against the working directory.

Read the page before you send it. It is exactly what the model saw.

## Recall earlier prompts

Press Up on an empty composer to walk back through your prompts from this and
earlier sessions. e keeps them in `~/.e/history.jsonl` and recalls the newest
1000.
