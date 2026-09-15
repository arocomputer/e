# Instructions

e reads instructions from `AGENTS.md` files and puts them in the system
prompt, wrapped as project instructions with the file's path:

- `~/.e/AGENTS.md` — yours, for every project.
- `<workspace>/AGENTS.md` — the project's, loaded once the directory is
  trusted (`/trust`, or `e trust [dir]` when there is no terminal). An
  untrusted repository cannot steer the agent.
- `<workspace>/<dir>/…/AGENTS.md` — nested instructions, loaded the first
  time a tool reads, writes, edits, or searches a path under that directory,
  as a message in the conversation. The nearest file arrives last, so it
  reads as the most specific. Each nested file loads once per session.

Nested files are how a monorepo keeps its rules local: `services/api/AGENTS.md`
is not in context until the agent touches `services/api/`, and then it is,
without anyone asking. They load only in a trusted workspace, and only for
paths inside it.

Files are capped at 32 KiB each. Reasoning, skills, and prompt templates
have their own guides (`e docs skills`, `e docs prompt-templates`).
