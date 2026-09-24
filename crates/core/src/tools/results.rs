//! `read_result`: page into a tool result the model saw truncated.
//!
//! Bash keeps the tail of a long command, grep and extension results are
//! capped, and each cut names the id under which the runtime kept the whole
//! text. This tool reads that text back by byte window, or as the lines
//! matching a query, so a long test log or a big search is never lost —
//! only deferred. Results live for the session and are bounded in number
//! and size; a request for an evicted id says so.

use serde_json::{json, Value};

use super::{ToolOutcome, ToolOutput, MAX_BYTES};

pub fn read_result_schema() -> Value {
    super::schema_object(
        "read_result",
        "Read more of a tool result that was truncated. A truncation notice names the result id. Returns a byte window (offset/limit, at most 32 KiB per call) or, with query, every line containing the query with its line number.",
        json!({
            "id": {"type": "integer", "description": "The result id from the truncation notice"},
            "offset": {"type": "integer", "description": "Byte offset to start from; default 0"},
            "limit": {"type": "integer", "description": "Max bytes to return; default and cap 32768"},
            "query": {"type": "string", "description": "Return only lines containing this text, numbered"}
        }),
        &["id"],
    )
}

/// Serve a kept result by id: the lines matching `query` when one is given,
/// otherwise the byte window at `offset`. Arguments are checked in order
/// (id, then that the id is kept, then limit and offset) so each failure
/// names the first thing wrong.
pub fn read_result(args: &Value, _cwd: &std::path::Path, state: &super::ToolRuntime) -> ToolOutput {
    let Some(id) = args["id"].as_u64() else {
        return failed("read_result: missing integer id".into(), "bad arguments");
    };
    let Some(full) = state.result(id) else {
        return failed(
            format!(
                "read_result: no result {id} — it was never kept, or was evicted; rerun the command"
            ),
            "not found",
        );
    };
    let bad_arguments =
        |message: String| failed(format!("read_result: {message}"), "bad arguments");
    // The same lenient integer reading as `read`: a numeric string or an
    // integral float silently ignored would re-serve the first window.
    let limit = match super::integer_arg(args, "limit") {
        Ok(limit) => limit
            .map(|n| n.min(MAX_BYTES as u64) as usize)
            .filter(|n| *n > 0)
            .unwrap_or(MAX_BYTES),
        Err(message) => return bad_arguments(message),
    };
    let offset = match super::integer_arg(args, "offset") {
        Ok(offset) => offset.unwrap_or(0),
        Err(message) => return bad_arguments(message),
    };
    match args["query"].as_str().filter(|q| !q.is_empty()) {
        Some(query) => matching_lines(&full, query, limit),
        None => byte_window(&full, id, offset, limit),
    }
}

/// Every line of `full` containing `query`, numbered from 1, until the next
/// row would pass `limit` bytes.
fn matching_lines(full: &str, query: &str, limit: usize) -> ToolOutput {
    let mut out = String::new();
    let mut matches = 0usize;
    let mut cut = false;
    for (index, line) in full.lines().enumerate() {
        if !line.contains(query) {
            continue;
        }
        matches += 1;
        let row = format!("{}\t{line}\n", index + 1);
        if out.len() + row.len() > limit {
            cut = true;
            break;
        }
        out.push_str(&row);
    }
    if cut {
        out.push_str("… [more matches beyond the byte limit; narrow the query]\n");
    }
    let content = if matches == 0 {
        format!("no lines contain {query:?}")
    } else {
        out
    };
    completed(content, format!("{matches} matches"))
}

/// Up to `limit` bytes of result `id` from `offset`, widened to character
/// boundaries, with a footer naming the next offset or the end.
fn byte_window(full: &str, id: u64, offset: u64, limit: usize) -> ToolOutput {
    let total = full.len();
    let Some(offset) = usize::try_from(offset).ok().filter(|o| *o < total) else {
        return failed(
            format!("offset {offset} is past the end of result {id} ({total} bytes)"),
            "past the end",
        );
    };
    let mut start = offset;
    while !full.is_char_boundary(start) {
        start -= 1;
    }
    // The window ends on a character boundary; when the limit lands inside
    // the very first character, the window grows to include it rather than
    // shrinking to nothing and pointing at its own offset.
    let mut end = (start + limit).min(total);
    while !full.is_char_boundary(end) {
        end -= 1;
    }
    while end <= start {
        end += 1;
        while !full.is_char_boundary(end) {
            end += 1;
        }
    }
    let mut content = full[start..end].to_string();
    content.push_str(&format!(
        "\n… [bytes {start}–{end} of {total}{}]",
        if end < total {
            format!("; continue with offset {end}")
        } else {
            "; end of result".to_string()
        }
    ));
    completed(content, format!("{} bytes", end - start))
}

/// A failed read with the model-facing `content` and the row's `summary`.
fn failed(content: String, summary: &str) -> ToolOutput {
    ToolOutput {
        content,
        outcome: ToolOutcome::Failed,
        summary: summary.into(),
        display: None,
    }
}

/// A served read.
fn completed(content: String, summary: String) -> ToolOutput {
    ToolOutput {
        content,
        outcome: ToolOutcome::Completed,
        summary,
        display: None,
    }
}
