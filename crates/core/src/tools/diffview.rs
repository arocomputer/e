//! Line diff for the detail viewer, in the reference row grammar:
//!
//! ```text
//!    41   fn before(&self) {
//!    42 -     let old = 1;
//!    42 +     let new = 2;
//!    43   }
//!       ⋯
//! ```
//!
//! `{lineno:>5} {op} {text}` — removed rows carry old-file numbers, added
//! and context rows new-file numbers; unchanged stretches beyond three
//! context lines fold into a `⋯` row. The rows are plain text; the viewer
//! colors the number-and-sign column at paint time.

/// Context lines kept around each change.
const CONTEXT: usize = 3;

/// LCS cell budget. Past it the diff still renders — the common prefix and
/// suffix are exact, and the middle is marked as one replacement.
const MAX_CELLS: usize = 4_000_000;

enum Op {
    /// Context, carrying its new-file line number (1-based) — the number an
    /// editor opening the file today would show.
    Keep(usize),
    Del(usize),
    Add(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Lf,
    CrLf,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DiffLine<'a> {
    text: &'a str,
    ending: Ending,
}

fn split_lines(text: &str) -> Vec<DiffLine<'_>> {
    text.split_inclusive('\n')
        .map(|line| {
            if let Some(text) = line.strip_suffix("\r\n") {
                DiffLine {
                    text,
                    ending: Ending::CrLf,
                }
            } else if let Some(text) = line.strip_suffix('\n') {
                DiffLine {
                    text,
                    ending: Ending::Lf,
                }
            } else {
                DiffLine {
                    text: line,
                    ending: Ending::None,
                }
            }
        })
        .collect()
}

fn shown_line(line: DiffLine<'_>) -> String {
    match line.ending {
        Ending::CrLf => format!("{}␍", line.text),
        _ => line.text.to_string(),
    }
}

/// Render the diff between two texts. Empty when nothing changed.
pub fn render(before: &str, after: &str) -> String {
    let old = split_lines(before);
    let new = split_lines(after);
    let ops = script(&old, &new);
    if !ops.iter().any(|op| !matches!(op, Op::Keep(..))) {
        return String::new();
    }

    // Which op rows survive: every change, plus CONTEXT of Keep around it.
    let mut keep = vec![false; ops.len()];
    for (i, op) in ops.iter().enumerate() {
        if !matches!(op, Op::Keep(..)) {
            let from = i.saturating_sub(CONTEXT);
            let to = (i + CONTEXT + 1).min(ops.len());
            for slot in keep.iter_mut().take(to).skip(from) {
                *slot = true;
            }
        }
    }

    let mut out = String::new();
    let mut elided = false;
    for (i, op) in ops.iter().enumerate() {
        if !keep[i] {
            if !elided {
                out.push_str("      ⋯\n");
                elided = true;
            }
            continue;
        }
        elided = false;
        let (row, ending) = match op {
            Op::Keep(n) => (
                format!("{:>5}   {}", n, shown_line(new[*n - 1])),
                new[*n - 1].ending,
            ),
            Op::Del(o) => (
                format!("{:>5} - {}", o, shown_line(old[*o - 1])),
                old[*o - 1].ending,
            ),
            Op::Add(n) => (
                format!("{:>5} + {}", n, shown_line(new[*n - 1])),
                new[*n - 1].ending,
            ),
        };
        out.push_str(&row);
        out.push('\n');
        if !matches!(op, Op::Keep(_)) && ending == Ending::None {
            out.push_str("      \\ No newline at end of file\n");
        }
    }
    out.pop();
    out
}

/// The edit script: trim the exact common prefix and suffix, then LCS over
/// the middle (or one replacement block when the middle is too large).
fn script(old: &[DiffLine<'_>], new: &[DiffLine<'_>]) -> Vec<Op> {
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let mid_old = &old[prefix..old.len() - suffix];
    let mid_new = &new[prefix..new.len() - suffix];

    let mut ops = Vec::with_capacity(old.len().max(new.len()));
    for i in 0..prefix {
        ops.push(Op::Keep(i + 1));
    }
    if mid_old.len().saturating_mul(mid_new.len()) <= MAX_CELLS {
        lcs_ops(mid_old, mid_new, prefix, &mut ops);
    } else {
        for (i, _) in mid_old.iter().enumerate() {
            ops.push(Op::Del(prefix + i + 1));
        }
        for (i, _) in mid_new.iter().enumerate() {
            ops.push(Op::Add(prefix + i + 1));
        }
    }
    for i in 0..suffix {
        ops.push(Op::Keep(new.len() - suffix + i + 1));
    }
    ops
}

/// Standard LCS table walk over the trimmed middle; deletions before
/// insertions inside a replaced run, the conventional order.
fn lcs_ops(old: &[DiffLine<'_>], new: &[DiffLine<'_>], offset: usize, ops: &mut Vec<Op>) {
    let (rows, cols) = (old.len(), new.len());
    let mut table = vec![0u32; (rows + 1) * (cols + 1)];
    for o in (0..rows).rev() {
        for n in (0..cols).rev() {
            table[o * (cols + 1) + n] = if old[o] == new[n] {
                table[(o + 1) * (cols + 1) + n + 1] + 1
            } else {
                table[(o + 1) * (cols + 1) + n].max(table[o * (cols + 1) + n + 1])
            };
        }
    }
    let (mut o, mut n) = (0, 0);
    while o < rows && n < cols {
        if old[o] == new[n] {
            ops.push(Op::Keep(offset + n + 1));
            o += 1;
            n += 1;
        } else if table[(o + 1) * (cols + 1) + n] >= table[o * (cols + 1) + n + 1] {
            ops.push(Op::Del(offset + o + 1));
            o += 1;
        } else {
            ops.push(Op::Add(offset + n + 1));
            n += 1;
        }
    }
    while o < rows {
        ops.push(Op::Del(offset + o + 1));
        o += 1;
    }
    while n < cols {
        ops.push(Op::Add(offset + n + 1));
        n += 1;
    }
}

/// Convert a unified diff (what `git diff` prints) into the row grammar
/// above, so an extension's diff paints like a built-in edit's: `-` rows
/// carry old-file numbers, `+` and context rows new-file numbers, hunks
/// are separated by `⋯`, and each file opens with its path on a plain
/// row. Lines that are not part of a hunk (`diff --git`, `index`, mode
/// changes, `Binary files … differ`) survive as plain rows only where they
/// say something a reader needs: the path and a binary note.
pub fn from_unified(diff: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut old_no: usize = 0;
    let mut new_no: usize = 0;
    let mut in_hunk = false;
    let mut hunks_in_file = 0usize;
    for raw in diff.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix("+++ ") {
            let path = rest.strip_prefix("b/").unwrap_or(rest);
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(path.to_string());
            in_hunk = false;
            hunks_in_file = 0;
            continue;
        }
        if line.starts_with("--- ") && !in_hunk {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            // `-a[,b] +c[,d] @@…`
            let mut parts = rest.split_whitespace();
            let old = parts.next().unwrap_or("-1").trim_start_matches('-');
            let new = parts.next().unwrap_or("+1").trim_start_matches('+');
            old_no = old
                .split(',')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            new_no = new
                .split(',')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            if hunks_in_file > 0 {
                out.push("      ⋯".to_string());
            }
            hunks_in_file += 1;
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            if line.starts_with("Binary files") {
                if !out.is_empty() {
                    out.push(String::new());
                }
                out.push(line.to_string());
            }
            continue;
        }
        if line.starts_with('\\') {
            // "\ No newline at end of file" — a fact about the last row,
            // not a row of its own.
            continue;
        }
        let (op, text) = match line.chars().next() {
            Some('+') => ('+', &line[1..]),
            Some('-') => ('-', &line[1..]),
            Some(' ') => (' ', &line[1..]),
            None => (' ', ""),
            // A stray line ends the hunk (git's own header for the next
            // file starts with `diff`).
            Some(_) => {
                in_hunk = false;
                continue;
            }
        };
        match op {
            '+' => {
                out.push(format!("{:>5} + {}", new_no, text));
                new_no += 1;
            }
            '-' => {
                out.push(format!("{:>5} - {}", old_no, text));
                old_no += 1;
            }
            _ => {
                out.push(format!("{:>5}   {}", new_no, text));
                new_no += 1;
                old_no += 1;
            }
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod unified_tests {
    use super::from_unified;

    #[test]
    fn unified_hunks_become_numbered_rows_with_elisions_between_them() {
        let diff = "diff --git a/f.txt b/f.txt\nindex 1..2 100644\n--- a/f.txt\n+++ b/f.txt\n\
@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n@@ -10,2 +10,3 @@\n x\n+y\n z\n\\ No newline at end of file\n\
diff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n";
        let rows = from_unified(diff);
        assert_eq!(
            rows,
            "f.txt\n    1   a\n    2 - b\n    2 + B\n    3   c\n      ⋯\n   10   x\n   11 + y\n   12   z\n\nBinary files a/img.png and b/img.png differ"
        );
    }

    #[test]
    fn text_that_is_not_a_diff_yields_nothing() {
        assert_eq!(from_unified("hello\nworld\n"), "");
    }
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn a_one_line_change_shows_numbers_context_and_markers() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let after = "a\nb\nc\nd\nE\nf\ng\nh\n";
        let out = render(before, after);
        // Old number on the removal, new number on the addition, three
        // context lines each side, the far edges elided.
        assert!(out.contains("    5 - e"), "{out}");
        assert!(out.contains("    5 + E"), "{out}");
        assert!(out.contains("    4   d"), "{out}");
        assert!(out.contains("    8   h"), "{out}");
        assert!(out.contains("      ⋯"), "{out}");
        assert!(!out.contains("  1   a"), "{out}");
    }

    #[test]
    fn identical_texts_render_nothing() {
        assert_eq!(render("same\n", "same\n"), "");
    }

    #[test]
    fn crlf_to_lf_is_visible() {
        let out = render("one\r\ntwo\r\n", "one\ntwo\n");
        assert!(out.contains("    1 - one␍"), "{out}");
        assert!(out.contains("    1 + one"), "{out}");
        assert!(!out.contains("    1 + one␍"), "{out}");
    }

    #[test]
    fn final_newline_changes_are_visible() {
        let removed = render("one\n", "one");
        assert!(removed.contains("    1 - one"), "{removed}");
        assert!(removed.contains("    1 + one"), "{removed}");
        assert!(
            removed.contains("\\ No newline at end of file"),
            "{removed}"
        );

        let added = render("one", "one\n");
        assert!(added.contains("    1 - one"), "{added}");
        assert!(added.contains("    1 + one"), "{added}");
        assert!(added.contains("\\ No newline at end of file"), "{added}");
    }

    #[test]
    fn a_new_file_is_all_additions() {
        let out = render("", "one\ntwo\n");
        assert!(out.contains("    1 + one"), "{out}");
        assert!(out.contains("    2 + two"), "{out}");
        assert!(!out.contains('-'), "{out}");
    }

    #[test]
    fn insertion_shifts_following_numbers() {
        let before = "a\nb\nc\n";
        let after = "a\nnew\nb\nc\n";
        let out = render(before, after);
        assert!(out.contains("    2 + new"), "{out}");
        // Context after the insertion carries new-file numbers.
        assert!(out.contains("    3   b"), "{out}");
        assert!(out.contains("    4   c"), "{out}");
    }

    #[test]
    fn oversized_middles_still_render_a_replacement() {
        // Force the fallback path with unique lines beyond the cell budget.
        let before: String = (0..2_100).map(|i| format!("x{i}\n")).collect();
        let after: String = (0..2_100).map(|i| format!("y{i}\n")).collect();
        let out = render(&before, &after);
        assert!(out.contains("    1 - x0"), "{}", &out[..200]);
        assert!(out.contains("    1 + y0"), "{}", &out[..200]);
    }
}
