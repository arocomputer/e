---
title: Sessions
description: Resume, branch, compact, and export a conversation.
order: 1
---

# Sessions

A session is one conversation with its history: messages, tool calls, their
output, and the usage they recorded.

Several features work on whole sessions:

- `ulo rpc` opens a session. See [automation](automation.md).
- `/resume` lists sessions.
- A [channel](channels.md) maps a session to a thread.

## Where they live

ulo saves each conversation as a JSONL file under `~/.ulo/sessions/`, one file per
session. ulo appends to the file as the turn runs.

| Command | Effect |
| --- | --- |
| `ulo` | start a new session |
| `ulo -c` | continue this directory's most recent session |
| `ulo -r` | pick a session from the list |

In the terminal, `/resume` does the same as `-r` with a picker. `/tree` shows
the current session's shape.

> [!IMPORTANT]
> Treat session files as project data. They hold what the tools read: source,
> configuration, and anything else the model was shown. A shared session or an
> [export](#export) is as sensitive as the repository.

## Branch in place

Branching lets you go back to an earlier message and try a different path.
Every message has an id and a parent, so a session is a tree, not a line.

`/tree` moves back to an earlier message and continues from there. The branch
you left stays in the file, and you can reach it the same way. Nothing is
rewritten.

Branching changes the conversation, not the working directory. It does not
undo edits a tool already made. Use git for the files.

### Fork a session

`/fork` copies the current branch into a session file of its own. Use it when
the next task deserves a separate history rather than a second branch of this
one.

`ulo rpc` exposes both, as `session.fork` and the `parent` of a resumed session.

## Context

When the model's context window fills, ulo summarizes older messages to make room
and continues the same turn. You do not restart the task.

Run `/compact` to ask for that summary by hand. Add a focus to steer what the
summary keeps, such as decisions, unfinished work, or a subsystem:

```
/compact <focus>
```

Compaction preserves the instructions and the previous summaries, so a long
session keeps its rules.

`/usage` reports recorded tokens and estimated cost by model for the session.
The report includes compaction's own requests.

## Export

`/export` writes the active branch as one self-contained HTML page. The page
holds the conversation, the tool activity, and the code it touched.

Read the page before you send it. It is exactly what the model saw.

## Recall

Press up on an empty composer to walk back through your prompts from this and
earlier sessions. ulo keeps them in `~/.ulo/history.jsonl`. This is usually faster
than remembering which session you meant.

`/undo` restores up to 100 session writes or edits.

## Commands

| Command | Effect |
| --- | --- |
| `/resume` | reopen a saved session |
| `/tree` | return to an earlier message and branch in place |
| `/fork` | continue in a separate session file |
| `/compact [focus]` | summarize older context, optionally around a focus |
| `/export` | save the active branch as one HTML page |
| `/undo` | restore session writes or edits |
| `/usage` | tokens and estimated cost per model |
| `/name <label>` | name the session, shown in `/resume` |
