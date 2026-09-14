//! Usage over time, from the sessions on disk: every provider response
//! carries its model, timestamp, and token counts in the session log, so a
//! period's spend is a fold over those records — local, exact for tokens,
//! estimated for cost from the current catalog's prices. Nothing is sent
//! anywhere.

use std::collections::BTreeMap;
use std::path::Path;

use crate::core::providers::{catalog, Usage};

/// One model's line in the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub model: String,
    pub requests: u64,
    pub usage: Usage,
    /// None when the model is not in the current catalog, so its prices
    /// are unknown.
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Default)]
pub struct Report {
    /// Rows by model, most requests first.
    pub rows: Vec<Row>,
    pub requests: u64,
    pub usage: Usage,
    /// Sum over rows with a known price; None when no row had one.
    pub cost_usd: Option<f64>,
}

/// A period argument: `24h`, `7d`, `30d`, or `all`. None for anything else.
pub fn period_ms(word: &str) -> Option<Option<u64>> {
    Some(match word.trim() {
        "" | "7d" | "week" => Some(7 * 86_400_000),
        "24h" | "day" | "today" => Some(86_400_000),
        "30d" | "month" => Some(30 * 86_400_000),
        "all" => None,
        _ => return None,
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Responses recorded since `since_ms` across every session file, folded
/// by model. `session_paths` lets the caller (and tests) scope the files.
pub fn report(session_paths: &[std::path::PathBuf], since_ms: Option<u64>) -> Report {
    let mut by_model: BTreeMap<String, Row> = BTreeMap::new();
    for path in session_paths {
        for response in crate::core::session::responses_in(path) {
            if since_ms.is_some_and(|since| response.timestamp < since) {
                continue;
            }
            let Some(usage) = response.usage else {
                continue;
            };
            let slug = format!("{}/{}", response.provider, response.model);
            let row = by_model.entry(slug.clone()).or_insert_with(|| Row {
                model: slug,
                requests: 0,
                usage: Usage::default(),
                cost_usd: None,
            });
            row.requests += 1;
            row.usage.add(usage);
        }
    }
    let mut report = Report::default();
    for (_, mut row) in by_model {
        row.cost_usd = catalog::catalog()
            .into_iter()
            .find(|m| catalog::slug(m) == row.model)
            .and_then(|m| m.pricing.map(|p| p.estimate(row.usage)));
        report.requests += row.requests;
        report.usage.add(row.usage);
        if let Some(cost) = row.cost_usd {
            report.cost_usd = Some(report.cost_usd.unwrap_or(0.0) + cost);
        }
        report.rows.push(row);
    }
    report
        .rows
        .sort_by_key(|row| std::cmp::Reverse(row.requests));
    report
}

/// The report for a period word over every saved session.
pub fn report_for(period: &str) -> Option<Report> {
    let window = period_ms(period)?;
    let since = window.map(|w| now_ms().saturating_sub(w));
    let paths: Vec<std::path::PathBuf> = crate::core::session::list_all()
        .into_iter()
        .map(|info| info.path)
        .collect();
    Some(report(&paths, since))
}

/// The report as a markdown table for a `show` block.
pub fn markdown(report: &Report, period_label: &str) -> String {
    use crate::core::output::{format_cost, format_tokens};
    if report.requests == 0 {
        return format!("No responses recorded in the {period_label}.");
    }
    let mut out = String::from("| model | requests | input | output | cache read | cost |\n|---|---:|---:|---:|---:|---:|\n");
    for row in &report.rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            row.model,
            row.requests,
            format_tokens(row.usage.input),
            format_tokens(row.usage.output),
            format_tokens(row.usage.cache_read),
            row.cost_usd.map(format_cost).unwrap_or_else(|| "—".into()),
        ));
    }
    out.push_str(&format!(
        "| **total** | {} | {} | {} | {} | {} |\n",
        report.requests,
        format_tokens(report.usage.input),
        format_tokens(report.usage.output),
        format_tokens(report.usage.cache_read),
        report
            .cost_usd
            .map(format_cost)
            .unwrap_or_else(|| "—".into()),
    ));
    out.push_str("\nCosts are estimates from the current catalog's prices; a model no longer listed shows —.");
    out
}

/// Whether `path` is a session log this report would read.
pub fn is_session_log(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "jsonl")
}
