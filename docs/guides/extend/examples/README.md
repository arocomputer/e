# Build extensions for ulo

Extensions add tools, commands, hooks, and interface elements to ulo without changing
its core. They run as separate processes and exchange JSONL messages with ulo over
stdin and stdout. ulo renders their output, owns keyboard focus, and reports process
failures. Use these working examples as starting points for your own integrations.

## What you can build

- **Commands and tools:** `hello.mjs` adds a greeting command and a tool the model
  can call. It also demonstrates session naming, input rewriting, and settings.
- **Planning workflows:** `plan.mjs` restricts the model to reading and searching,
  adds planning instructions, and shows an interactive checklist in a side pane.
- **Tool-call checks:** `gate.mjs` blocks selected destructive shell commands;
  `protected.mjs` blocks arguments that name sensitive paths. These examples inspect
  arguments only and fail open if the extension fails. They are not a sandbox.
- **MCP tools:** `mcp.mjs` connects an MCP stdio server and exposes its tools to ulo.
  It does not bridge MCP prompts, resources, sampling, or elicitation.
- **Delegated tasks:** `subagent.mjs` runs an isolated turn in another ulo process
  through `ulo rpc`, with a chosen model and tool allowlist.
- **Project startup:** `project.mjs` adds `--project <path>` and relaunches ulo in the
  selected directory before the session starts.

## Try an example

The examples run with Node.js. From the repository root, install the greeting
example and its protocol helper:

```sh
mkdir -p ~/.ulo/extensions/hello
cp docs/guides/extend/examples/hello.mjs docs/guides/extend/examples/scaffold.mjs ~/.ulo/extensions/hello/
chmod +x ~/.ulo/extensions/hello/hello.mjs
```

Restart ulo and enter `/hello`. The helper `scaffold.mjs` handles initialization,
request routing, and replies, so your extension can define handlers. The MCP and
subagent examples implement the protocol directly and do not need that helper.
Read each example's configuration before enabling it.

For the protocol and supported interactions, see the [extension guide](../extensions.md).
For sharing an extension with its prompts, skills, or themes, see [packages](../packages.md).
