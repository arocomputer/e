//! e-diff: Git workspace review as an e extension — the first package of
//! `packages/`, proof that the surfaces around e can live outside it.
//!
//! The host spawns this executable from `~/.e/extensions/e-diff` and speaks
//! the JSONL line protocol (docs/extensions.md): each stdin line is a
//! request carrying an `id`; each stdout line answers it. One request at a
//! time, sequentially — the review scan is bounded to five seconds, well
//! inside the host's sixty-second command budget.
//!
//! `/diff` answers with a `show` block: the whole review as a unified diff
//! that e paints in its own row grammar through the user's theme;
//! `/diff <path>` shows one file's patch. The host sanitizes notices, so
//! rows this crate styled itself would arrive plain; the display surface
//! is how the review gets colour.

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Stay under the host's one-megabyte line budget with room for JSON
/// escaping; an over-long review reports its own truncation.
const MAX_SHOW_BYTES: usize = 600_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

/// The line loop: read a request, answer it, until stdin closes.
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    // Initialized state: where to review.
    let mut cwd = std::env::current_dir().unwrap_or_default();

    while let Some(line) = lines.next_line().await? {
        // Events and flags carry no id and want no answer; malformed lines
        // are unanswerable by definition. Skipping both is the protocol.
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let response = match request["method"].as_str().unwrap_or("") {
            "initialize" => {
                if let Some(dir) = request["params"]["cwd"].as_str() {
                    cwd = PathBuf::from(dir);
                }
                ok(
                    id,
                    json!({
                        "name": "diff",
                        "version": env!("CARGO_PKG_VERSION"),
                        "description": "Git workspace review in the transcript",
                        "commands": [{
                            "name": "diff",
                            "description": "review uncommitted changes — /diff or /diff <path>",
                        }],
                    }),
                )
            }
            "command" => {
                let name = request["params"]["name"].as_str().unwrap_or("");
                let args = request["params"]["args"].as_str().unwrap_or("");
                match review(&cwd, name, args).await {
                    Ok(show) => ok(id, json!({ "show": show })),
                    Err(reason) => error(id, reason),
                }
            }
            "shutdown" => ok(id, Value::Null),
            method => error(id, format!("e-diff does not handle {method}")),
        };
        if write_line(&mut out, &response).is_err() {
            break;
        }
    }
    Ok(())
}

/// Build one review: the full workspace, or one file's patch, as a `show`.
async fn review(cwd: &Path, name: &str, args: &str) -> Result<Value, String> {
    if name != "diff" {
        return Err(format!("e-diff only owns /diff, not /{name}"));
    }
    let args = args.trim();
    let document = if args.is_empty() {
        e_diff::diff::load_review(cwd).await?
    } else {
        let selected = Path::new(args);
        let snapshot = e_diff::diff::load(cwd, Some(selected)).await?;
        let picked = snapshot
            .selected
            .filter(|path| *path == selected)
            .ok_or_else(|| format!("no changed file at {args}"))?;
        let file = snapshot
            .files
            .iter()
            .find(|file| file.path == picked)
            .cloned()
            .ok_or_else(|| format!("no changed file at {args}"))?;
        e_diff::diff::Review {
            files: vec![file],
            patches: vec![(picked.clone(), snapshot.patch)],
            truncated: false,
        }
    };
    let mut shown = e_diff::command::show(&document);
    if let Some(body) = shown["body"].as_str() {
        if body.len() > MAX_SHOW_BYTES {
            // The host clips long bodies itself; ending on a whole line here
            // keeps the last hunk the host paints a real one.
            let cut = body[..MAX_SHOW_BYTES].rfind('\n').unwrap_or(MAX_SHOW_BYTES);
            let clipped = body[..cut].to_string();
            shown["body"] = Value::String(clipped);
            if let Some(title) = shown["title"].as_str() {
                shown["title"] =
                    Value::String(format!("{title} — clipped; /diff <path> reviews one file"));
            }
        }
    }
    Ok(shown)
}

fn ok(id: Value, result: Value) -> String {
    json!({ "id": id, "result": result }).to_string()
}

fn error(id: Value, reason: String) -> String {
    json!({ "id": id, "error": reason }).to_string()
}

fn write_line(out: &mut impl Write, line: &str) -> std::io::Result<()> {
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}
