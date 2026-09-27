---
title: Prompt templates
description: Turn a markdown file into a /name command with arguments.
order: 4
---

# Prompt templates

A prompt template is a reusable prompt you run as a slash command. Use one
for a request you type often, with arguments filled in each time.

## Create a template

Save `~/.e/prompts/review.md`:

```markdown
---
description: review the changes
argument-hint: [path]
---
Review ${1:-everything} carefully. Focus on $2.
```

Now `/review src/ "error handling"` submits `Review src/ carefully. Focus on
error handling.` The file stem is the command name. e reads templates on
each use, so a new or edited file works immediately.

## Front matter

Front matter is optional.

| Key | Shown |
| --- | --- |
| `description` | In the `/` picker. |
| `argument-hint` | After the description. |

## Arguments

e splits the text after the command into words on whitespace. Single or
double quotes group words into one argument. The body is then submitted with
these substitutions:

| Syntax | Expands to |
| --- | --- |
| `$1` … `$9` | One positional argument, or nothing. |
| `$@` or `$ARGUMENTS` | All arguments, joined with spaces. |
| `${N}` | Argument N, for any N. |
| `${N:-default}` | Argument N, or `default` when it is missing or empty. |
| `${@:-default}` | All arguments, or `default` when there are none. |
| `${@:N}` | The arguments from the Nth on. |

Any other `$` stays literal.

## Where templates come from

| Location | Loads |
| --- | --- |
| `~/.e/prompts/<name>.md` | Always. |
| `prompts/<name>.md` in an installed [package](../extend/packages.md) | Unless the home has a template of the same name. |
| `<workspace>/.e/prompts/<name>.md` | Once the workspace is [trusted](instructions.md#trust). It shadows both of the others, because the closer context wins. |
