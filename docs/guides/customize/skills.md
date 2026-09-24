---
title: Skills
description: SKILL.md folders the model reads in when a task needs them.
order: 5
---

# Skills

A skill is a directory with a `SKILL.md` file: `~/.ulo/skills/<name>/SKILL.md`.
It follows the open SKILL.md convention, so other tools can read the same
files.

```markdown
---
name: release
description: how to cut a release of this project
---
Step one …
```

ulo reads skill files on each use. Add or edit a skill and it is live
immediately.

## How the model uses a skill

ulo advertises a catalog in the system prompt. Each entry holds the skill's
name, its description, and the path to its `SKILL.md`. When the task matches,
the model reads the body in with the ordinary `read` tool. Until then, only
the descriptions stay in context.

A `description:` may span lines. Use a `>` or `|` block scalar, or an indented
continuation. It folds to one line in the catalog and the picker.

## Insert a skill by hand

The `$` picker inserts a skill by hand.

`disable-model-invocation: true` keeps a skill out of the catalog. Only the
`$` picker reaches it.

## Package skills

An installed [package](../extend/packages.md) contributes its `skills/`
directory the same way. A global skill shadows a package's skill of the same
name. The `$` picker labels package skills `Package`.

## Repo-local skills

A trusted repository can carry its own skills in `.ulo/skills/`, as
`<repo>/.ulo/skills/<name>/SKILL.md`, in the same format as above.

These skills load only after `/trust`, like the repo's AGENTS.md. They shadow
a global skill of the same name, because the closer context wins.
