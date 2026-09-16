//! Session events and their stable JSON representation for frontend consumers.

use super::failure;
use crate::core::{providers, tools};
use providers::FailureCause;

/// One call in a provider-issued tool batch: the call as the model made it
/// (`name`, raw JSON `arguments`) plus the labels a transcript shows for it.
#[derive(Clone, Debug)]
pub struct ToolCallPresentation {
    pub id: u64,
    pub name: String,
    pub arguments: String,
    pub category: String,
    pub running: String,
    pub completed: String,
    pub target: String,
}

impl SessionEvent {
    /// The event as one JSON object for the `-p --json` stream and other
    /// machine consumers: a `type` tag plus the event's fields. Liveness
    /// ticks (`ToolCallAssembly`) are not events a consumer acts on, so
    /// they serialize to None.
    pub fn to_json(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        Some(match self {
            SessionEvent::TurnStart => json!({"type": "turn_start"}),
            SessionEvent::Discarded(prompts) => json!({"type": "discarded", "prompts": prompts}),
            SessionEvent::Compacting => json!({"type": "compacting"}),
            SessionEvent::Compacted {
                summary,
                context_tokens,
                ..
            } => json!({"type": "compacted", "summary": summary, "context_tokens": context_tokens}),
            SessionEvent::TextDelta(delta) => json!({"type": "text", "delta": delta}),
            SessionEvent::ReasoningDelta(delta) => json!({"type": "reasoning", "delta": delta}),
            SessionEvent::ToolBatchStart { calls } => json!({
                "type": "tool_batch",
                "calls": calls.iter().map(|call| json!({
                    "id": call.id,
                    "name": call.name,
                    "arguments": serde_json::from_str::<serde_json::Value>(&call.arguments)
                        .unwrap_or_else(|_| serde_json::Value::String(call.arguments.clone())),
                    "category": call.category,
                    "target": call.target,
                })).collect::<Vec<_>>(),
            }),
            SessionEvent::ToolStart { id } => json!({"type": "tool_start", "id": id}),
            SessionEvent::ToolOutput { id, stream, chunk } => json!({
                "type": "tool_output",
                "id": id,
                "stream": format!("{stream:?}").to_lowercase(),
                "chunk": chunk,
            }),
            SessionEvent::ToolEnd {
                id,
                outcome,
                summary,
                content,
            } => json!({
                "type": "tool_end",
                "id": id,
                "outcome": format!("{outcome:?}").to_lowercase(),
                "summary": summary,
                "content": content,
            }),
            SessionEvent::ToolCallAssembly { .. } => return None,
            SessionEvent::Named(name) => json!({"type": "session_name", "name": name}),
            SessionEvent::Instructions { path } => json!({"type": "instructions", "path": path}),
            SessionEvent::Usage { usage, .. } => json!({
                "type": "usage",
                "input_tokens": usage.input,
                "output_tokens": usage.output,
                "cache_read_tokens": usage.cache_read,
                "cache_write_5m_tokens": usage.cache_write_5m,
                "cache_write_1h_tokens": usage.cache_write_1h,
            }),
            SessionEvent::ErrorDetails(details) => {
                json!({"type": "error_details", "details": details.as_ref()})
            }
            SessionEvent::Error(message) => json!({"type": "error", "message": message}),
            SessionEvent::Warning(message) => json!({"type": "warning", "message": message}),
            SessionEvent::Retry {
                attempt,
                limit,
                delay_secs,
                cause,
                reason,
            } => json!({
                "type": "retry",
                "attempt": attempt,
                "limit": limit,
                "delay_secs": delay_secs,
                "cause": cause.label(),
                "reason": reason,
            }),
            SessionEvent::Recovered { attempt, limit } => {
                json!({"type": "recovered", "attempt": attempt, "limit": limit})
            }
            SessionEvent::Steered(text) => json!({"type": "steered", "text": text}),
            SessionEvent::Slept { duration_secs } => {
                json!({"type": "slept", "duration_secs": duration_secs})
            }
            SessionEvent::SleepStopped { duration_secs } => {
                json!({"type": "sleep_stopped", "duration_secs": duration_secs})
            }
            SessionEvent::TurnEnd { aborted } => json!({"type": "turn_end", "aborted": aborted}),
        })
    }
}

#[derive(Debug)]
pub enum SessionEvent {
    TurnStart,
    /// Queued prompts that could not run after cancellation or worker failure.
    Discarded(Vec<String>),
    /// Context maintenance belongs to the core, including headless runs.
    Compacting,
    Compacted {
        summary: String,
        context_tokens: u64,
        response: providers::ResponseMeta,
        /// Rates captured from the model that made the request.
        pricing: Option<providers::catalog::Pricing>,
    },
    TextDelta(String),
    ReasoningDelta(String),
    /// All calls from one assistant message, known before concurrent execution.
    ToolBatchStart {
        calls: Vec<ToolCallPresentation>,
    },
    /// One member of the batch now owns execution focus.
    ToolStart {
        id: u64,
    },
    /// A command pipe chunk observed before process completion.
    ToolOutput {
        id: u64,
        stream: tools::OutputStream,
        chunk: String,
    },
    /// A tool finished with a typed outcome and bounded full output.
    ToolEnd {
        id: u64,
        outcome: tools::ToolOutcome,
        summary: String,
        content: String,
    },
    /// Cumulative bytes of tool-call argument JSON streamed so far this
    /// step. Argument assembly is the one long stream phase with no other
    /// event — without this the UI freezes while the turn is alive.
    ToolCallAssembly {
        bytes: u64,
    },
    /// An extension tool named the session.
    Named(String),
    /// A nested `AGENTS.md` under a path a tool touched was added to the
    /// conversation (docs/guides/customize/instructions.md).
    Instructions {
        path: String,
    },
    Usage {
        usage: providers::Usage,
        /// Rates captured from the model that made the request.
        pricing: Option<providers::catalog::Pricing>,
    },
    /// Diagnostic facts emitted immediately before the compatible Error message.
    ErrorDetails(Box<failure::ErrorDetails>),
    Error(String),
    /// A non-fatal turn problem worth showing: a truncated or refused reply
    /// the provider delivered as success, or malformed stream frames that
    /// were skipped.
    Warning(String),
    /// A retryable failure is being backed off before another attempt.
    Retry {
        attempt: u32,
        limit: u32,
        delay_secs: u64,
        cause: FailureCause,
        reason: String,
    },
    /// The first attempt after one or more retries produced something —
    /// shown briefly before the row reverts to normal turn activity.
    Recovered {
        attempt: u32,
        limit: u32,
    },
    /// A steering message was accepted mid-turn (for display as a user block).
    Steered(String),
    /// The process was suspended and woke within the resume window; the
    /// turn is being continued automatically over the committed partial.
    Slept {
        duration_secs: u64,
    },
    /// The process was suspended longer than the resume window; the turn
    /// stopped. Partial work is committed; this is a stop, not an error.
    SleepStopped {
        duration_secs: u64,
    },
    TurnEnd {
        aborted: bool,
    },
}
