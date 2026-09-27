---
title: Sandboxing
description: How far e trusts a session, and how to isolate one.
order: 4
---

# Sandboxing

e has no built-in permission system: it runs with the permissions of the
user and process that launch it. Read this guide before you point e at code
or credentials you need to protect. Isolation has to come from outside e.

## What a session can reach

The `read`, `write`, `edit`, and `bash` tools can touch anything the e process
can:

- the whole filesystem
- any network the machine can reach
- any credential in the environment

This is deliberate. e stays small and leaves isolation to tools built for it.

## Isolate a session

For a real boundary, isolate the process.

**Run e in a container.** Run `e` inside Docker or an equivalent, and scope
the mounts, network, and credentials to what the session needs. This is the
coarsest option and the simplest to reason about. Start here unless you need
finer control.

**Use a restricted user or a VM.** Run `e` as a low-privilege user, or inside
a VM. Choose this when a container's shared kernel is not isolation enough
for your threat model.

**Sandbox only `bash`.** An extension tool with a built-in's name replaces
that built-in (see [extensions](../extend/extensions.md#initialize)).
An extension can therefore replace `bash` with a version that wraps each
command in an OS sandbox before running it, such as:

- `bwrap` on Linux
- `firejail` on Linux
- `sandbox-exec` on macOS

e ships no example of this. Sandbox flags are easy to get subtly wrong: a
missing `--unshare-net`, a bind mount that is writable when it should be
read-only, or a loose `sandbox-exec` profile makes a setup look isolated when
it is not. If you build one:

- Wrap a maintained sandboxing tool rather than hand-rolling flags.
- When that tool is missing, fail loudly and refuse the call. Never fall back
  to running the command unsandboxed.
- Keep the built-in `bash` schema's full contract: `command`, `timeout`,
  `background`, `handle`, and `signal` (see
  [`crates/core/src/tools/bash.rs`](../../../crates/core/src/tools/bash.rs)).
  Reject what you do not support with a clear error instead of silently
  dropping it. A tool that accepts `background: true` and then hangs is worse
  than one that says "not supported here".

## Gate tool calls with a hook

The `tool_call` hook lets an extension block individual tool calls before
they run. See [`hook.tool_call`](../extend/extensions.md#results-by-method)
and two examples:

- [`gate.mjs`](../extend/examples/gate.mjs) denies destructive bash patterns.
- [`protected.mjs`](../extend/examples/protected.mjs) denies
  credential-shaped paths.

A hook is a useful speed bump, not a boundary. It fails open by design: a
slow or broken gate never blocks the agent (see
[timeouts and failures](../extend/extensions.md#timeouts-and-failures)). It
does not hold against:

- a compromised or adversarial extension
- a model that finds a pattern the denylist missed
- a bug in the hook itself

Treat the hook as a second layer on top of real isolation.

## Extensions and packages

Extensions run as your user. The rest of the extension surface (events, the
`before_turn` and `tool_result` hooks, and the `ui.*` and `session.*`
requests) has the same posture: an extension can narrow the toolset with
`session.tools`, append to the system prompt, redact tool output, and ask the
user things, and none of that contains it. See
[extensions](../extend/extensions.md).

An extension cannot emit terminal bytes, rewrite the provider request, or
replace the system prompt, and every request it makes is bounded and
answered. That protects e's own integrity, not the machine.

Packages installed with `e install` widen this exposure. A package's
extensions launch as your user like any other extension, and its skills and
prompts steer the model. Installing clones the package with your own `git`
and runs nothing; everything runs at the next launch. Read a package before
you install it, and pin the ref you read, such as `@v1`. See
[packages](../extend/packages.md).

## Local storage

On Unix, e keeps its own files private from other local accounts:

- the home directory is made owner-only when e writes to it or reopens a
  session
- session logs are created owner-only
- credential files are staged owner-only before secrets are written

These permissions do not protect against tools or extensions running as your
own user. [Compatibility](../extend/compatibility.md) covers how e handles
older session files.

## The build guard is not a sandbox

`scripts/guard.sh` audits e's own source and build, not a running session. It
checks the allowed network hosts, the `~/.e` home, where `unsafe` code lives,
and SHA-pinned CI actions. A clean `guard.sh` says nothing about what a live
`e` process can reach.
