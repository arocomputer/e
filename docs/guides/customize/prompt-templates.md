---
title: Prompt templates
description: Turn a markdown file into a /name command with arguments.
order: 4
---

# Prompt templates

A prompt template is a reusable prompt you run as a slash command. The
markdown file `~/.ulo/prompts/<name>.md` becomes the `/name` command.

```markdown
---
description: review the changes
argument-hint: [path]
---
Review ${1:-everything} carefully. Focus on $2.
```

ulo reads templates on each use, so it picks up new files immediately.

## Front matter

- `description` shows in the `/` picker.
- `argument-hint` shows after the description.

## Arguments

ulo submits the body as the prompt after bash-style substitution. Quoted
arguments group as one word.

| Syntax | Expands to |
| --- | --- |
| `$1`..`$9` | The positional argument. |
| `$@` or `$ARGUMENTS` | All arguments. |
| `${N:-default}` | Argument N, or `default` when it is missing or empty. |
| `${@:-default}` | All arguments, or `default` when there are none. |
| `${@:2}` | The arguments from the 2nd on. |

## Package templates

An installed [package](../extend/packages.md) contributes its `prompts/`
directory the same way. A global template shadows a package's template of the
same name.

## Repo-local templates

A trusted repository can carry its own commands in `.ulo/prompts/`.
`<repo>/.ulo/prompts/<name>.md` becomes `/name`, in the same format as above.

These templates load only after `/trust`, like the repo's AGENTS.md. They
shadow a global template of the same name, because the closer context wins.
