# Keep response provenance outside model history

Status: accepted
Date: 2026-09-13

## Context

Version 1 stored partial usage inside assistant messages. Its input counter mixed
cached and uncached tokens, omitted cache-write duration, inherited the session
header's model after model changes, and acquired a new message identity when
compaction copied recent history. Compaction requests had no durable accounting.
The same object was also replayed to providers, coupling local diagnostics to
provider-facing conversation data.

## Decision

Session format version 2 adds an optional response envelope beside the replayable
message. The envelope has a stable local response id and completion timestamp,
provider, model, purpose (`turn` or `compaction`), and disjoint uncached input,
output, cache-read, five-minute cache-write, and one-hour cache-write counters.
It is restored as out-of-band message metadata in memory and excluded from every
provider dialect's serialization. Copying retained messages preserves the
response id; a compaction summary carries the summarization response on its seed.

Readers continue to accept versions 0 and 1 without rewriting them. Their old
inline usage is ignored by the core rather than translated into counters whose
missing provenance and cache-write categories cannot be recovered.

## Consequences

A resumed session can identify which model handled each recorded response, and
local accounting can deduplicate responses copied by compaction. Compaction cost
joins ordinary turn cost in TUI and RPC totals. RPC usage categories are now
disjoint and include the reconstructed complete prompt total.

Version 1 binaries reject newly written version 2 sessions instead of silently
replaying a shape they do not understand. Existing sessions need no migration
and remain readable by the new version.
