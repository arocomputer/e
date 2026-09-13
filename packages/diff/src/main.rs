//! e-diff: Git workspace review as an e extension — the first package of
//! `packages/`, proof that the surfaces around e can live outside it.
//!
//! The host spawns this executable from `~/.e/extensions/e-diff` and speaks
//! the JSONL line protocol (docs/extensions.md): each stdin line is a
//! request carrying an `id`; each stdout line answers it. One request at a
//! time, sequentially — the review scan is bounded to five seconds, well
//! inside the host's sixty-second command budget.
//!
//! `/diff` prints the whole continuous review (file summaries plus every
//! patch) as styled transcript rows; `/diff <path>` prints one file's patch.
//! Layout, syntax coloring, and palette come from this crate — the same
//! `e-terminal` primitives the host renders with, so the review looks like
//! e without being part of e.

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Transcript rows are rendered at a fixed width and re-wrapped by the
/// host; user-overridable through `diff_text_width` in `~/.e/settings.json`.
const DEFAULT_WIDTH: u64 = 78;
/// Stay under the host's one-megabyte line budget with room for JSON
/// escaping; an over-long review reports its own truncation.
const MAX_NOTICE_BYTES: usize = 600_000;

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
    // Initialized state: where to review, and what the user configured.
    let mut cwd = std::env::current_dir().unwrap_or_default();
    let mut config = json!({});

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
                config = request["params"]["extensions_config"].clone();
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
                match review(&cwd, &config, name, args).await {
                    Ok(notice) => ok(id, json!({ "notice": notice })),
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

/// Build one review document: the full workspace, or one file's patch.
async fn review(cwd: &Path, config: &Value, name: &str, args: &str) -> Result<String, String> {
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
    // Respect the host's theme selection (a file-backed setting; user theme
    // directories stay the host's own privilege until extensions can ask).
    let light = config.get("theme").and_then(Value::as_str) == Some("light");
    let theme = e_diff::style::theme(light, &Value::Null);
    let width = config
        .get("diff_text_width")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_WIDTH)
        .clamp(40, 120) as usize;
    let mut panel = e_diff::diffpanel::DiffPanel::new(config);
    panel.apply(Ok(document));
    let mut notice = String::new();
    let mut dropped = 0usize;
    for row in panel.document(&theme, width) {
        if notice.len() + row.len() > MAX_NOTICE_BYTES {
            dropped += 1;
        } else {
            if !notice.is_empty() {
                notice.push('\n');
            }
            notice.push_str(&row);
        }
    }
    if dropped > 0 {
        notice.push_str(&format!(
            "\n… {dropped} more rows omitted — /diff <path> reviews one file"
        ));
    }
    Ok(notice)
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
