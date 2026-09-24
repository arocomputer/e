//! The result of one headless turn: what `ulo -p --json` prints last and what
//! `ulo rpc` answers a prompt with. Folded from the session event stream so
//! every headless consumer reports a turn the same way.

use ulo_core::agent::SessionEvent;
use ulo_core::providers;

#[derive(Default)]
pub struct TurnAccumulator {
    pub output: String,
    pub error: Option<String>,
    pub error_details: Option<ulo_core::agent::failure::ErrorDetails>,
    pub warnings: Vec<String>,
    pub aborted: bool,
    /// `TurnEnd` arrived; the run has stopped.
    pub terminal: bool,
    pub usage: providers::Usage,
    pub tool_calls: u64,
    pub tool_failures: u64,
    unended: std::collections::HashSet<u64>,
}

impl TurnAccumulator {
    pub fn with_warnings(warnings: Vec<String>) -> Self {
        Self {
            warnings,
            ..Self::default()
        }
    }

    pub fn observe(&mut self, event: &SessionEvent) {
        match event {
            SessionEvent::TextDelta(delta) => self.output.push_str(delta),
            SessionEvent::ToolBatchStart { calls } => {
                self.tool_calls += calls.len() as u64;
                self.unended.extend(calls.iter().map(|call| call.id));
            }
            SessionEvent::ToolEnd { id, outcome, .. } => {
                if self.unended.remove(id) && outcome.is_error() {
                    self.tool_failures += 1;
                }
            }
            SessionEvent::Compacted { response, .. } => {
                if let Some(usage) = response.usage {
                    self.usage.add(usage);
                }
            }
            SessionEvent::Usage { usage, .. } => self.usage.add(*usage),
            SessionEvent::Warning(warning) => self.warnings.push(warning.clone()),
            SessionEvent::Retry {
                attempt,
                limit,
                delay_secs,
                cause,
                reason,
            } => self.warnings.push(format!(
                "{} — retrying ({attempt}/{limit}) in {delay_secs}s: {reason}",
                cause.label()
            )),
            SessionEvent::ErrorDetails(details) => {
                self.error_details = Some(details.as_ref().clone())
            }
            SessionEvent::Error(message) => self.error = Some(message.clone()),
            SessionEvent::TurnEnd { aborted } => {
                self.tool_failures += self.unended.len() as u64;
                self.unended.clear();
                self.aborted = *aborted;
                self.terminal = true;
            }
            _ => {}
        }
    }

    /// The stream closed: a turn that never reached `TurnEnd` is an error,
    /// not a silent success.
    pub fn finish(&mut self) {
        if !self.terminal && self.error.is_none() {
            self.error = Some("agent event stream closed before turn completion".into());
        }
    }

    pub fn failed(&self) -> bool {
        self.error.is_some() || self.aborted
    }

    /// The result object. `final_output` is the reply only when the turn
    /// completed cleanly; `output` is whatever streamed either way.
    pub fn json(
        &self,
        selected_model: &str,
        effort: Option<&str>,
        pricing: Option<&providers::catalog::Pricing>,
    ) -> serde_json::Value {
        let final_output = if self.failed() {
            ""
        } else {
            self.output.as_str()
        };
        serde_json::json!({
            "output": self.output,
            "final_output": final_output,
            "model": selected_model,
            "effort": effort,
            "aborted": self.aborted,
            "error": self.error,
            "error_details": self.error_details,
            "warnings": self.warnings,
            "usage": {
                "input_tokens": self.usage.input,
                "output_tokens": self.usage.output,
                "cache_read_tokens": self.usage.cache_read,
                "cache_write_5m_tokens": self.usage.cache_write_5m,
                "cache_write_1h_tokens": self.usage.cache_write_1h,
                "prompt_tokens": self.usage.prompt_tokens(),
            },
            "cost_usd": pricing.map(|rates| rates.estimate(self.usage)),
            "tools": {"calls": self.tool_calls, "failures": self.tool_failures},
        })
    }
}
