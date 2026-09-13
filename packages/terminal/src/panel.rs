//! Optional side-panel protocol. The host owns placement, clipping and input routing.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Layout preferences returned by a command. Hosts clamp them to usable bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Options {
    pub min_width: usize,
    pub width_percent: usize,
    pub refresh_ms: u64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            min_width: 110,
            width_percent: 40,
            refresh_ms: 1000,
        }
    }
}

/// A pointer event in panel-local, zero-based terminal cells.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Mouse {
    pub kind: String,
    pub column: u16,
    pub row: u16,
    pub shift: bool,
}

/// One frame request, with ordered pointer events since the previous request.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Request {
    pub width: usize,
    pub height: usize,
    pub light: bool,
    pub colors: HashMap<String, serde_json::Value>,
    pub events: Vec<Mouse>,
}

/// One replaceable composer attachment. Labels are plain text; content is sent
/// only when the user submits the draft. Neither field is a command to execute.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Attachment {
    pub label: String,
    pub content: String,
}

/// A frame may update the draft or close its own panel. Rows accept SGR styling
/// only; the host strips other terminal controls and bounds the result.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Frame {
    pub rows: Vec<String>,
    pub attachment: Option<Attachment>,
    pub close: bool,
}

impl Frame {
    /// Reject oversized replies before they reach layout or the composer.
    pub fn validate(&self) -> Result<(), String> {
        if self.rows.len() > 500 || self.rows.iter().map(String::len).sum::<usize>() > 256 * 1024 {
            return Err("panel frame exceeds 500 rows or 256 KiB".into());
        }
        if self.attachment.as_ref().is_some_and(|a| {
            a.label.len() > 1024 || a.content.len() > 64 * 1024 || a.label.trim().is_empty()
        }) {
            return Err("panel attachment requires a label and at most 64 KiB of content".into());
        }
        Ok(())
    }
}
