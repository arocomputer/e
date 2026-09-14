//! The `/diff` command's answer on e's display surface: a `show` block with
//! `format: "diff"`, which the host converts to its own row grammar and
//! paints through the user's theme. Rows styled here would be sanitized at
//! the host boundary, so the review crosses the line as a unified diff.

use serde_json::{json, Value};

use crate::diff::Review;

/// One review as a `show` object: the title carries the file count and
/// line totals (and a truncation note); the body is every patch, each
/// preceded by the file headers the host needs to label it.
pub fn show(review: &Review) -> Value {
    let files = review.files.len();
    let added: usize = review.files.iter().filter_map(|f| f.added).sum();
    let removed: usize = review.files.iter().filter_map(|f| f.removed).sum();
    let mut title = format!(
        "{files} file{} changed +{added} -{removed}",
        if files == 1 { "" } else { "s" }
    );
    if review.truncated {
        title.push_str(" (truncated)");
    }
    let mut body = String::new();
    for (path, patch) in &review.patches {
        if !body.is_empty() {
            body.push('\n');
        }
        let shown = path.to_string_lossy();
        if !patch.lines().any(|line| line.starts_with("+++ ")) {
            body.push_str(&format!("--- a/{shown}\n+++ b/{shown}\n"));
        }
        body.push_str(patch);
        if !patch.ends_with('\n') {
            body.push('\n');
        }
    }
    if body.is_empty() {
        return json!({"title": "no changes", "body": "clean working tree", "format": "text"});
    }
    json!({"title": title, "body": body, "format": "diff"})
}
