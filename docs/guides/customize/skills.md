---
title: Skills
description: SKILL.md folders the model reads in when a task needs them.
order: 5
---

# Skills

A skill is a folder of instructions, and any files they reference, that the
model reads only when a task calls for it. Use one for a procedure too long
or too rare to keep in [AGENTS.md](instructions.md).

## Create a skill

Save `~/.e/skills/release/SKILL.md`:

```markdown
---
name: release
description: how to cut a release of this project
---
Step one …
```

Files the body references go beside `SKILL.md` in the same folder. Skills
follow the open SKILL.md convention, so other tools can read the same files.
e reads them on each use, so a new or edited skill works immediately.

## Front matter

| Key | Default | Meaning |
| --- | --- | --- |
| `name` | the folder name | The skill's name in the catalog and the picker. |
| `description` | empty | When the skill applies. The model decides from this alone. |
| `disable-model-invocation` | `false` | `true` keeps the skill out of the catalog, so only you can use it. |

A `description` may span lines, as a `>` or `|` block scalar or an indented
continuation. It folds to one line.

## How the model uses a skill

The system prompt carries a catalog: each skill's name, description, and the
path to its `SKILL.md`. When a task matches, the model reads the file with
the ordinary `read` tool. Until then only the catalog line is in context.

## Use a skill yourself

Type `$` in the composer to open the skills picker. Each row shows where the
skill comes from (Global, Workspace, or Package), and Tab filters by source.
Choosing a skill submits its body, with the skill's folder path and any text
you typed before the `$`, as your prompt.

## Where skills come from

| Location | Loads |
| --- | --- |
| `~/.e/skills/<name>/SKILL.md` | Always. |
| `skills/<name>/SKILL.md` in an installed [package](../extend/packages.md) | Unless the home has a skill of the same name. |
| `<workspace>/.e/skills/<name>/SKILL.md` | Once the workspace is [trusted](instructions.md#trust). It shadows both of the others, because the closer context wins. |
