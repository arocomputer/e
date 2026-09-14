//! Prompt history across sessions: `~/.e/history.jsonl`, one JSON string
//! per line, newest last. The composer seeds its up-arrow recall from the
//! tail at launch and every submitted prompt is appended. Prompts are the
//! user's own words, so the file is private (0600) and never read by the
//! model; secrets typed into the API-key field never reach it because that
//! path does not record history.

use std::io::Write as _;

use crate::core::config::home;

/// Entries the composer recalls; older ones stay in the file until a trim.
pub const RECALL: usize = 1000;
/// Past this many lines the file is rewritten to its newest [`RECALL`].
const TRIM_AT: usize = 2 * RECALL;

fn path() -> std::path::PathBuf {
    home::home().join("history.jsonl")
}

/// The newest `limit` prompts, oldest first. A missing or unreadable file
/// is an empty history; a line that is not a JSON string is skipped.
pub fn load(limit: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    let entries: Vec<String> = text
        .lines()
        .filter_map(|line| serde_json::from_str::<String>(line).ok())
        .filter(|entry| !entry.trim().is_empty())
        .collect();
    let skip = entries.len().saturating_sub(limit);
    entries.into_iter().skip(skip).collect()
}

/// Append one prompt. Empty text and an exact repeat of the last entry are
/// not recorded. Failures are silent: history is a convenience, and a
/// read-only home must not cost a turn.
pub fn append(entry: &str) {
    if entry.trim().is_empty() {
        return;
    }
    let path = path();
    if let Some(last) = load(1).pop() {
        if last == entry {
            return;
        }
    }
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    let _ = home::ensure();
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(&path) {
        let _ = writeln!(file, "{line}");
    }
    trim(&path);
}

/// Keep the file bounded: past `TRIM_AT` lines, rewrite it as its newest
/// `RECALL`. Counted by newline, so a trim reads the file once in a while
/// rather than on every append.
fn trim(path: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let lines = text.lines().count();
    if lines <= TRIM_AT {
        return;
    }
    let kept: Vec<&str> = text.lines().skip(lines - RECALL).collect();
    let staged = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(&staged) {
        if writeln!(file, "{}", kept.join("\n")).is_ok() {
            let _ = std::fs::rename(&staged, path);
        }
    }
}
