---
title: Sandboxing
description: How far ulo trusts a session, and how to isolate one.
order: 4
---

# Sandboxing

ulo has no built-in permission system. It runs with the permissions of the user
and process that launch it. The isolation has to come from outside ulo.

## What a session can reach

The `read`, `write`, `edit`, and `bash` tools can touch anything the ulo process
can:

- the whole filesystem
- any network the machine can reach
- any credential in the environment

This is deliberate. ulo stays small and leaves isolation to tools built for it,
which the rest of this guide covers.

## Local storage

Local storage still has privacy boundaries. On Unix, ulo does the following:

- makes its state home private on writes and session reopening
- creates session logs owner-only
- creates credential staging files owner-only before writing secrets

These permissions protect against other local accounts. They do not protect
against tools or extensions running as your own user. See
[compatibility](../extend/compatibility.md) for how ulo handles older session
files.

## The build guard

`scripts/guard.sh` is a different thing and does not cover this. It audits
*ulo's own build*, not the permissions of a running session. It checks:

- allowed network hosts
- the `~/.ulo` home
- where `unsafe` lives
- SHA-pinned CI actions

A clean `guard.sh` says nothing about what a live `ulo` process can reach.

## Extensions

The rest of the extension surface has the same posture. That surface includes
events, the `before_turn` and `tool_result` hooks, and the `ui.*` and
`session.*` requests. See [extensions](../extend/extensions.md).

An extension can do these things, and none of them contains it:

- narrow the toolset with `session.tools`
- append to the system prompt
- redact tool output
- ask the user things

An extension cannot emit terminal bytes, rewrite the provider request, or
replace the system prompt. Every request it makes is bounded and answered. That
boundary protects ulo's own integrity, not the machine's.

## Packages

Packages installed with `ulo install` widen this exposure. They do not narrow
it. A package's extensions launch as your user like any other extension, and
its skills and prompts steer the model.

Installing a package clones it with your own `git`. Nothing runs at install
time. Everything runs at the next launch.

Read a package before you install it, and pin the ref you read, such as `@v1`.

## The `tool_call` hook

The `tool_call` hook is what ulo gives you. Extensions can use it to gate
individual tool calls. See [extensions](../extend/extensions.md#results-by-method)
and two examples:

- [`gate.mjs`](../extend/examples/gate.mjs) denies destructive bash patterns.
- [`protected.mjs`](../extend/examples/protected.mjs) denies credential-shaped
  paths.

A hook is a real, useful speed bump. It is also fail-open by design. A slow or
broken tool gate never blocks the agent, as
[timeouts and failures](../extend/extensions.md#timeouts-and-failures)
describes.

A hook guards against clearly bad, anticipated patterns. It is not a boundary
that holds against any of these:

- a compromised or adversarial extension
- a model that finds a pattern the denylist missed
- a bug in the hook itself

Treat the hook as a second layer, not the isolation.

## Getting a real boundary

For an actual boundary, isolate the process.

**Container the whole session.** Run `ulo` itself inside Docker or an equivalent.
Scope the mounts, network, and credentials to what the session needs. This is
the coarsest-grained option and the simplest to reason about. It is the default
recommendation if you don't need finer control.

**Use a restricted user or VM.** Run `ulo` as a low-privilege user, or inside a
VM. Choose this when a container's shared kernel isn't isolation enough for
your threat model.

**Sandbox just `bash` at the OS level.** ulo's extension protocol lets a tool
declaration override a built-in by using its name. See
[extensions](../extend/extensions.md). An extension can replace `bash` with a
version that wraps the command before running it, using one of these:

- `bwrap` on Linux
- `firejail`
- `sandbox-exec` on macOS

There is no example of this in `docs/guides/extend/examples/` yet. It is real
work to get right, as the next section explains. The mechanism exists today.

If you build one, preserve the built-in bash schema's full contract:
`command`, `timeout`, `background`, `handle`, and `signal`. See
`crates/core/src/tools/bash.rs`. Explicitly reject what you don't support rather than
silently dropping it. A tool that claims to support `background: true` and
then hangs or errors opaquely is worse than one that says plainly "not
supported here."

## Why there's no built-in sandbox example yet

Hand-rolled sandbox flags are easy to get subtly wrong. Examples include
bwrap's bind-mount list, a seccomp filter, and a `sandbox-exec` profile. A
mistake can make a setup *look* isolated when it isn't, such as a missing
`--unshare-net` or a bind mount that's writable when it should be read-only.

Getting that right is its own project. `thule` is the planned first-party
answer. It is built to give ulo a real execution boundary directly, instead of
leaving every user to wrap `bash` themselves.

Until `thule` lands, a future `docs/guides/extend/examples/sandbox.mjs` should
build on the `tool_call`-override mechanism above. It should wrap a maintained
sandboxing tool. When that tool isn't installed, it should fail loudly and
refuse the call. It should never fall back to running the command unsandboxed.
