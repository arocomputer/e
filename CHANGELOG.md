# e

## Unreleased

**Review Git changes beside your conversation with `/diff`, and read full tool output in a redesigned Ctrl+O reader. Pasted text is easier to identify and remove. Cancellation, compaction, and session saving have also been hardened.**

### Breaking changes

- `e ask` is removed. Use `e rpc` for headless automation, with one JSON request and response per line. Piped stdin without `e rpc` now reports a usage error.
- Read-only tool mode is removed, including `--read-only`, `--ro`, the `read_only` RPC mode, and `read_only_notice`. Use `--no-tools` to disable tools, or the RPC `tools` allowlist to select built-ins.
- The `ask` tool and its question panel are removed. Extensions should read required input from configuration or report what is missing.
- Ctrl+D no longer quits from an empty composer. Press Ctrl+C twice to exit. Ctrl+D deletes forward and can be rebound in `~/.e/keybindings.json`.
- Unlabeled code fences no longer guess a language. Markdown footnotes render as literal text.
- The unstable Rust API now uses `core::extensions`, `SessionLog`, and tagged `MessageKind` payloads. Role-specific message fields become accessors.

### New features

- `/diff` opens a mouse-driven review document beside chat, with syntax colors, word-level changes, and wrapped source. Drag source to add a blue inline attachment without leaving the composer. Selecting again replaces its snapshot; two Backspaces remove it.
- Ctrl+O adopts fx's full-output reader layout, with wrapped output, vertical rails, and a navigation footer. Scroll with the keyboard or mouse; End resumes following new output.
- Extensions can live in directories under `~/.e/extensions/`, keeping their entry point and helper files together.
- `e rpc` accepts a built-in `tools` allowlist and returns the saved `session` path. The subagent example uses these for delegated tasks and access to their full results.
- `e help` prints the same usage as `e --help`.

### Improvements

- Consecutive successful edits to the same file share one transcript row with cumulative counts. Any intervening tool call breaks the group. Ctrl+O and session history keep every call. Set `combine_consecutive_edits` to `"off"` in `~/.e/settings.json` to keep separate rows.

- Pasted-text labels show their draft-local number and character count in the same dim gray as image attachments. Set `paste_placeholder` in `~/.e/settings.json` to change the collapse threshold; `0` inserts pastes literally.
- Running tools stay connected to their tree while output streams. Multiline commands keep dim continuation rows, and completed commands replace previews with their retained output.
- Long replies no longer copy the entire response on each update. Live Markdown rendering has a fixed work budget.
- RPC continues through automatic compaction and returns the final answer. `/compact` uses the same core path at the next provider boundary.
- Compaction preserves user instructions and earlier summaries. Truncated or ineffective summaries leave history intact and report an error.
- Session writers use OS-held locks. Stop older processes before resuming their sessions; existing JSONL files need no migration. Empty lock sidecars may remain.
- File edits preserve symlink targets, hard links, ACLs, and extended attributes. A failed final copy can still leave a partial update.
- Independent agents can use separate configuration homes and workspaces. File observations and background-process handles belong to each agent.
- The default system prompt asks for concise answers and clear file paths.
- Changelogs and GitHub releases use short summaries and grouped bullets, without an appended install block.

### Bug fixes

- Deleting a pasted-text or diff marker discards its hidden payload. Diff markers select on the first Backspace and delete on the second. History and completion preserve attachments that remain in the draft, and CRLF pastes no longer gain extra newlines.
- Ctrl+O no longer opens blank after long conversations. Closing either reader restores the main terminal buffer without adding expanded output to chat scrollback.
- Long tool labels, including image paths, fit their column without a one-cell overflow.
- Cancelled runs skip queued tools, and late tool events cannot change a newer turn. Continuous shell output no longer starves timeout checks.
- Tool batches have bounded concurrency. Calls that name the same file run in provider order.
- Compaction rejects stale history snapshots and checks cancellation before replacing history.
- Session resume locks the log before reading it. Corrupt parent links are rejected even on inactive branches.
- Failed turn and paint workers report errors instead of leaving the activity row running silently.
- A screenshot sent to a model without image support keeps your question and drops only the image. Rejected-image text remains literal even if it begins with a command.
- Shift+Tab moves backward through an open picker's tabs without changing reasoning effort behind it.
- Explicit `context_window` overrides survive live model refreshes and e updates.
- Editing a queued prompt no longer pauses the queue. If the turn has already consumed it, saving the edit submits a new prompt.
- Grep no longer skips the line after an oversized line. Provider usage totals saturate instead of overflowing.
- OpenAI reasoning summaries preserve paragraph breaks and stay hidden unless Show thinking is enabled.

### Security

- Provider and OAuth requests refuse redirects, keeping credentials and private request bodies on the intended origin. Release downloads reject HTTPS downgrades.
- Credential staging files are private from creation. On Unix, e's home uses `0700` and session files use `0600`, including older files when reopened.
- Tool labels, targets, and live previews strip terminal control sequences.
- OAuth callbacks have size and time limits. Malformed or idle connections no longer end login, and cancellation interrupts accepted connections.
- File writes detect inode replacement during staging and copying on Unix.
- SIGTERM and SIGHUP stop all tracked built-in shell process groups before `e rpc` exits.
- Git review disables external diffs, text conversion, filesystem monitors, and clean/process filters. It uses bounded reads and does not write the index.

## 0.0.1

**The first e release brings a Rust coding agent to macOS and Linux. Use hosted or local models, resume and branch conversations, and add your own tools, commands, and themes.**

### Breaking changes

- `ls`, `find`, and the dedicated `skill` tool are removed. Use bash for directory listings and filename searches, and `read` to load a skill's `SKILL.md`.
- OpenCode Zen's provider ID is now `opencode-zen`. Existing `opencode` credentials still work; saved model selections under that ID need to be selected once more.

### New features

- Connect to OpenAI, Anthropic, Google, xAI, OpenCode, OpenRouter, and other hosted providers, or use Ollama and LM Studio locally.
- Sign in with supported subscriptions or API keys. The model picker refreshes live catalogs and lets you save a preferred model scope.
- Resume with `e -c` or `/resume`, and branch from an earlier prompt with `/tree`. Older linear sessions load without conversion.
- Steer an active turn by sending another message. Review queued prompts above the composer.
- Attach PNG, JPEG, GIF, and WebP images. Supported models receive them, and sessions retain them for resume.
- Open full tool output and edit diffs with Ctrl+O. Run shell commands directly with `!`.
- Use `/compact` or automatic compaction to continue long conversations. The previous session remains available in `/resume`.
- Add tools, slash commands, hooks, and typed launch flags through executable JSONL extensions. Examples include MCP tools, delegated agents, and worktree launches.
- Customize themes, composer keybindings, models, prompt templates, and skills under `~/.e/`. Trusted projects can supply their own instructions, skills, and prompts.
- Use `e ask` for a headless turn or `e rpc` for JSONL automation. `e doctor` and `e providers` provide redacted diagnostics.
- Install checksum-verified binaries for macOS and Linux on ARM64 or x86-64. `e update` downloads updates, and `/reload` switches to an installed update while resuming your session.
- Read the bundled configuration and extension guides with `e docs`.

### Improvements

- Tool calls render as connected trees with live command output, outcome counts, and edit statistics. Full write content stays in Ctrl+O rather than filling the conversation.
- Markdown supports highlighted code, tables, task lists, nested blockquotes, and terminal hyperlinks. The composer wraps drafts, supports selection, and collapses large pastes.
- Provider retries show their cause and backoff and can be cancelled immediately. Billing and quota failures stop without retrying; `retry_max_attempts` controls the retry budget.
- Sessions record timestamps and real token usage. Footer counts use provider reports rather than estimated output, and Show thinking is off by default.
- Reasoning effort reaches supported models across all four provider dialects. Anthropic prompt caching covers the growing conversation.
- Write and edit results send a short confirmation to the model instead of repeating the new content. Long shell results retain their output tail.
- Model, skill, and session pickers have filter tabs. `/help` opens the searchable command picker.
- Terminal rendering preserves native scrollback and redraws changed rows. Long syntax-highlighted lines no longer stall input.

### Bug fixes

- Resume follows the selected branch rather than replaying abandoned messages. Torn final records can be recovered; interior corruption reports an error.
- Interrupted replies retain text already shown, and unfinished tool calls get explicit results so history can replay.
- Provider streams preserve split UTF-8, reasoning blocks, and tool-call identity. Truncated or filtered replies report warnings instead of appearing successful.
- Shell commands enforce their timeout, stop their process group, and drain output without deadlocking. Non-regular files fail instead of hanging tools.
- Edits reject stale file observations, and parallel edits to the same file no longer silently overwrite each other.
- Extension crashes and blocked pipes cannot leave calls waiting indefinitely. Failed startup cleans up child processes.
- Startup preserves typed input, and exit paths restore terminal modes. Wide characters and long drafts keep the cursor in the right column.
- Session and configuration write failures report warnings. Unknown configuration keys survive updates, and corrupt configuration files are preserved for recovery.

### Security

- Directory trust controls project instructions, skills, and prompts, not tool execution. Trusted ancestors cover their children unless a workspace has its own recorded choice.
- Model output and extension notices cannot inject terminal controls. Hyperlinks reject control bytes and oversized URLs.
- Pasted API keys stay out of composer recall history. Diagnostics omit credential values.
- Releases include checksums, a CycloneDX SBOM, and signed build provenance. CI checks allowed network hosts, configuration write paths, and pinned workflow actions.

[Detailed development history](https://github.com/intuitums/e/blob/48d1e0cba4665ca6c2f9050d27a510f3bfa989aa/CHANGELOG.md).
