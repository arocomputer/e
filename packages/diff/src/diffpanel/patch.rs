//! Unified patches projected as source rows, with bounded word-level comparisons.

use crate::style::strip_ansi;
use crate::style::{highlight_diff_block, theme::Theme};
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

/// A source row keeps its original text for attachments, apart from display styling.
#[derive(Debug)]
pub(super) struct Row {
    pub number: Option<usize>,
    pub kind: char,
    pub text: String,
    pub display: String,
    pub words: Vec<Range<usize>>,
}

impl Row {
    fn new(number: Option<usize>, kind: char, text: &str) -> Self {
        let mut display = String::new();
        let mut column = 0;
        for c in strip_ansi(text)
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
        {
            if c == '\t' {
                let spaces = 4 - column % 4;
                display.push_str(&" ".repeat(spaces));
                column += spaces;
            } else {
                display.push(c);
                column += c.width().unwrap_or(0);
            }
        }
        Self {
            number,
            kind,
            text: text.into(),
            display,
            words: Vec::new(),
        }
    }
}

/// Drop Git headers, count old/new lines independently, and separate disjoint hunks.
pub(super) fn parse(source: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut old, mut new) = (0usize, 0usize);
    let mut in_hunk = false;
    let has_hunks = source.lines().any(|line| line.starts_with("@@ "));
    for line in source.lines() {
        if line.starts_with("@@ ") {
            let mut fields = line.split_whitespace().skip(1);
            let number = |field: Option<&str>, sign: char| {
                field?
                    .strip_prefix(sign)?
                    .split(',')
                    .next()?
                    .parse::<usize>()
                    .ok()
            };
            if let (Some(a), Some(b)) = (number(fields.next(), '-'), number(fields.next(), '+')) {
                if in_hunk {
                    rows.push(Row::new(None, '…', "⋯"));
                }
                (old, new) = (a, b);
                in_hunk = true;
                continue;
            }
        }
        if in_hunk {
            match line.as_bytes().first() {
                Some(b'+') => {
                    rows.push(Row::new(Some(new), '+', &line[1..]));
                    new = new.saturating_add(1);
                }
                Some(b'-') => {
                    rows.push(Row::new(Some(old), '-', &line[1..]));
                    old = old.saturating_add(1);
                }
                Some(b' ') => {
                    rows.push(Row::new(Some(new), ' ', &line[1..]));
                    old = old.saturating_add(1);
                    new = new.saturating_add(1);
                }
                _ => rows.push(Row::new(None, '!', line)),
            }
        } else if !has_hunks
            && !["diff --git ", "index ", "--- ", "+++ "]
                .iter()
                .any(|prefix| line.starts_with(prefix))
        {
            rows.push(Row::new(None, '!', line));
        }
    }
    // Each replacement run pairs removed and added rows in order. Unpaired rows
    // retain their whole-line background without inventing an inline comparison.
    let mut at = 0;
    let mut budget = 1_000_000usize;
    while at < rows.len() {
        if rows[at].kind != '-' {
            at += 1;
            continue;
        }
        let removed = at;
        while at < rows.len() && rows[at].kind == '-' {
            at += 1;
        }
        let added = at;
        while at < rows.len() && rows[at].kind == '+' {
            at += 1;
        }
        for offset in 0..(added - removed).min(at - added) {
            let a = removed + offset;
            let b = added + offset;
            let (before, after) = changed_words(&rows[a].display, &rows[b].display, &mut budget);
            rows[a].words = before;
            rows[b].words = after;
        }
    }
    rows
}

/// Highlight each hunk's old and new source separately, so a deleted comment
/// opener cannot tint the added code. Cache these width-independent rows in the panel.
pub(super) fn syntax(rows: &[Row], theme: &Theme, lang: &str) -> Vec<String> {
    let mut out: Vec<String> = rows.iter().map(|r| r.display.clone()).collect();
    let mut start = 0;
    while start < rows.len() {
        let end = rows[start..]
            .iter()
            .position(|r| r.kind == '…')
            .map_or(rows.len(), |n| start + n);
        for removed in [true, false] {
            let indices: Vec<usize> = (start..end)
                .filter(|i| {
                    rows[*i].number.is_some() && rows[*i].kind != if removed { '+' } else { '-' }
                })
                .collect();
            let source = indices
                .iter()
                .map(|i| rows[*i].display.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            for (i, styled) in indices
                .into_iter()
                .zip(highlight_diff_block(theme, lang, &source))
            {
                if !removed || rows[i].kind == '-' {
                    out[i] = styled;
                }
            }
        }
        start = end + 1;
    }
    out
}

/// Identifier, whitespace, and punctuation tokens carry character offsets, not bytes.
fn tokens(text: &str) -> Vec<(String, Range<usize>)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let start = at;
        let word = chars[at].is_alphanumeric() || matches!(chars[at], '_' | '$');
        let space = chars[at].is_whitespace();
        at += 1;
        while at < chars.len()
            && ((word && (chars[at].is_alphanumeric() || matches!(chars[at], '_' | '$')))
                || (space && chars[at].is_whitespace()))
        {
            at += 1;
        }
        out.push((chars[start..at].iter().collect(), start..at));
    }
    out
}

/// Word LCS with per-line and per-patch cell budgets. Oversized pairs highlight
/// their changed middle after trimming equal tokens, rather than blocking input.
fn changed_words(
    before: &str,
    after: &str,
    budget: &mut usize,
) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let (a, b) = (tokens(before), tokens(after));
    let mut prefix = 0;
    while prefix < a.len().min(b.len()) && a[prefix].0 == b[prefix].0 {
        prefix += 1;
    }
    let (mut end_a, mut end_b) = (a.len(), b.len());
    while end_a > prefix && end_b > prefix && a[end_a - 1].0 == b[end_b - 1].0 {
        end_a -= 1;
        end_b -= 1;
    }
    let (n, m) = (end_a - prefix, end_b - prefix);
    let cells = (n + 1).saturating_mul(m + 1);
    let (mut keep_a, mut keep_b) = (vec![false; n], vec![false; m]);
    if cells <= 65_536 && cells <= *budget {
        *budget -= cells;
        let mut lcs = vec![0u16; cells];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * (m + 1) + j] = if a[prefix + i].0 == b[prefix + j].0 {
                    1 + lcs[(i + 1) * (m + 1) + j + 1]
                } else {
                    lcs[(i + 1) * (m + 1) + j].max(lcs[i * (m + 1) + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if a[prefix + i].0 == b[prefix + j].0 {
                keep_a[i] = true;
                keep_b[j] = true;
                i += 1;
                j += 1;
            } else if lcs[(i + 1) * (m + 1) + j] >= lcs[i * (m + 1) + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    let changed = |tokens: &[(String, Range<usize>)], keep: &[bool]| {
        let mut spans: Vec<Range<usize>> = Vec::new();
        for ((_, range), keep) in tokens.iter().zip(keep) {
            if *keep {
                continue;
            }
            if let Some(last) = spans.last_mut().filter(|last| last.end == range.start) {
                last.end = range.end;
            } else {
                spans.push(range.clone());
            }
        }
        spans
    };
    (
        changed(&a[prefix..end_a], &keep_a),
        changed(&b[prefix..end_b], &keep_b),
    )
}

/// Paint a horizontally scrolled code row. Only renderer-produced SGR is accepted;
/// clipping uses display cells while word ranges use characters. The caller closes the row background.
pub(super) fn code(
    theme: &Theme,
    styled: &str,
    words: &[Range<usize>],
    skip: usize,
    width: usize,
    base: &str,
    emphasis: &str,
) -> String {
    let mut out = String::new();
    let mut chars = styled.chars().peekable();
    let default_fg = match theme.fg_prefix("diffText") {
        "" => "\x1b[39m",
        prefix => prefix,
    };
    let mut fg = default_fg.to_string();
    let mut active_fg = String::new();
    let mut active_word = None;
    let (mut index, mut column, mut used, mut span) = (0, 0, 0, 0);
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            let mut sgr = String::from(c);
            for next in chars.by_ref() {
                sgr.push(next);
                if next == 'm' {
                    break;
                }
            }
            fg = if sgr == "\x1b[39m" {
                default_fg.to_string()
            } else {
                sgr
            };
            continue;
        }
        let cells = c.width().unwrap_or(0);
        while span < words.len() && words[span].end <= index {
            span += 1;
        }
        let word = words.get(span).is_some_and(|r| r.contains(&index));
        index += 1;
        column += cells;
        if column <= skip {
            continue;
        }
        let visible_cells = cells.min(column - skip);
        if used + visible_cells > width {
            break;
        }
        if active_word != Some(word) {
            // bg() restores the terminal default even for partial custom themes.
            out.push_str(&theme.bg("", ""));
            out.push_str(&theme.bg_prefix(if word { emphasis } else { base }));
            active_word = Some(word);
        }
        if fg != active_fg {
            out.push_str(&fg);
            active_fg.clone_from(&fg);
        }
        if column - cells < skip {
            out.push_str(&" ".repeat(visible_cells));
            used += visible_cells;
        } else {
            out.push(c);
            used += cells;
        }
    }
    out.push_str(&theme.bg("", ""));
    out.push_str(&theme.bg_prefix(base));
    out.push_str(&" ".repeat(width.saturating_sub(used)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_numbers_and_copy_text_do_not_include_patch_headers() {
        let rows = parse("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -8,2 +10,2 @@\n keep\n-old\n+new\n@@ -40 +42 @@\n-last\n+next");
        assert_eq!(
            rows.iter().filter_map(|r| r.number).collect::<Vec<_>>(),
            [10, 9, 11, 40, 42]
        );
        assert_eq!(rows[0].text, "keep");
        assert_eq!(rows[3].kind, '…');
    }

    #[test]
    fn word_highlights_skip_unchanged_syntax_and_support_unicode() {
        let rows = parse(
            "@@ -1 +1 @@\n-  if (差 < 60000) return 'old';\n+  if (差 < MINUTE) return 'now';",
        );
        let spans = |r: &Row| {
            r.words
                .iter()
                .map(|range| {
                    r.display
                        .chars()
                        .skip(range.start)
                        .take(range.len())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(spans(&rows[0]), ["60000", "old"]);
        assert_eq!(spans(&rows[1]), ["MINUTE", "now"]);
    }

    #[test]
    fn exhausted_word_budget_still_marks_only_the_changed_middle() {
        let (a, b) = changed_words("let before = 1;", "let after = 2;", &mut 0);
        assert_eq!(a, vec![4..14]);
        assert_eq!(b, vec![4..13]);
    }

    #[test]
    fn staged_new_files_hide_metadata_but_keep_original_indentation() {
        let rows = parse("diff --git a/a b/a\nnew file mode 100644\nindex 000..abc\n--- /dev/null\n+++ b/a\n@@ -0,0 +1 @@\n+\t界");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "\t界");
        assert_eq!(rows[0].display, "    界");
        assert_eq!(rows[0].number, Some(1));
    }

    #[test]
    fn deleted_comment_openers_do_not_tint_added_source() {
        let theme = crate::style::theme(false, &serde_json::json!({}));
        let rows = parse("@@ -1 +1 @@\n-/* old\n+const n = 1;");
        let styled = syntax(&rows, &theme, "ts");
        assert!(styled[0].contains(theme.fg_prefix("diffSyntaxComment")));
        assert!(styled[1].contains(&theme.fg("diffSyntaxKeyword", "const")));
        assert!(!styled[1].contains(theme.fg_prefix("diffSyntaxComment")));
    }

    #[test]
    fn horizontal_crop_keeps_partial_wide_cells_and_combining_marks() {
        let theme = crate::style::theme(false, &serde_json::json!({}));
        let paint = |skip, width| {
            code(
                &theme,
                "界e\u{301}x",
                &[],
                skip,
                width,
                "diffPaneBg",
                "diffAddedWordBg",
            )
        };
        assert_eq!(strip_ansi(&paint(1, 1)), " ");
        assert_eq!(strip_ansi(&paint(1, 3)), " e\u{301}x");
        assert_eq!(strip_ansi(&paint(2, 2)), "e\u{301}x");
    }

    #[test]
    fn partial_themes_restore_default_ink_after_syntax() {
        let theme =
            Theme::from_json(r##"{"vars":{},"colors":{"diffSyntaxKeyword":"#ff0000"}}"##).unwrap();
        let styled = format!("{} n", theme.fg("diffSyntaxKeyword", "const"));
        let painted = code(&theme, &styled, &[], 0, 7, "diffPaneBg", "diffAddedWordBg");
        assert!(painted.contains("const\x1b[39m n"), "{painted:?}");
    }
}
