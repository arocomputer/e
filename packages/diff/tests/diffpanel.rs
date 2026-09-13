//! The review document's navigation and output contract: selection becomes
//! owned prompt context, refresh keeps snapshots stable, and untrusted
//! source can never escape the frame it is rendered into.
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use e_diff::diff::{File, Review};
use e_diff::diffpanel::{Action, DiffPanel};
use serde_json::json;

fn theme() -> e_diff::style::theme::Theme {
    e_diff::style::theme(false, &json!({}))
}

/// One source document, with paths independent from display escaping.
fn review(patches: &[(&str, &str)]) -> Review {
    Review {
        files: patches
            .iter()
            .map(|(path, _)| File {
                path: path.into(),
                added: Some(1),
                removed: Some(1),
                new: false,
            })
            .collect(),
        patches: patches
            .iter()
            .map(|(path, patch)| ((*path).into(), (*patch).into()))
            .collect(),
        truncated: false,
    }
}

fn mouse(kind: MouseEventKind, row: usize) -> MouseEvent {
    MouseEvent {
        kind,
        column: 8,
        row: row as u16,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn release_attaches_source_once_per_logical_line_and_refresh_keeps_the_snapshot() {
    use MouseEventKind::*;
    let mut panel = DiffPanel::new(&json!({}));
    let source =
        "@@ -0,0 +1 @@\n+\tconst 界 = 'a long line that wraps across several display rows';";
    panel.apply(Ok(review(&[("src/lib/time.ts", source)])));
    let rows = panel.render(&theme(), 35, 30);
    let first = rows.iter().position(|row| row.contains("const")).unwrap();
    let last = rows.iter().position(|row| row.contains("rows")).unwrap();
    assert!(last > first);
    assert!(matches!(
        panel.mouse(mouse(Down(MouseButton::Left), first)),
        Action::None
    ));
    assert!(matches!(
        panel.mouse(mouse(Drag(MouseButton::Left), last)),
        Action::None
    ));
    let Action::Attach { label, content } = panel.mouse(mouse(Up(MouseButton::Left), last)) else {
        panic!("source attachment");
    };
    assert_eq!(label, "⧉ 1 line from diff");
    assert_eq!(content, "Selected lines from src/lib/time.ts:\n\tconst 界 = 'a long line that wraps across several display rows';\n");
    // A refresh replaces the pane's snapshot; the attached text above is
    // already owned by whoever received it and never re-reads the panel.
    panel.apply(Ok(review(&[("src/lib/time.ts", "@@ -0,0 +1 @@\n+later")])));
    assert!(!panel
        .render(&theme(), 35, 30)
        .iter()
        .any(|row| row.contains("wraps across")));
}

#[test]
fn refresh_during_a_drag_cannot_attach_different_source() {
    use MouseEventKind::*;
    let mut panel = DiffPanel::new(&json!({}));
    panel.apply(Ok(review(&[("a.rs", "@@ -1 +1 @@\n-before\n+after")])));
    let rows = panel.render(&theme(), 60, 30);
    let row = rows.iter().position(|row| row.contains("before")).unwrap();
    panel.mouse(mouse(Down(MouseButton::Left), row));
    panel.mouse(mouse(Drag(MouseButton::Left), row + 1));
    panel.apply(Ok(review(&[("a.rs", "@@ -1 +1 @@\n-before\n+later")])));
    assert!(matches!(
        panel.mouse(mouse(Up(MouseButton::Left), row + 1)),
        Action::None
    ));
}

#[test]
fn file_summaries_jump_and_wheel_scrolls_one_continuous_document() {
    use MouseEventKind::*;
    let mut panel = DiffPanel::new(&json!({}));
    panel.apply(Ok(review(&[
        (
            "a.rs",
            "@@ -0,0 +1,12 @@\n+a1\n+a2\n+a3\n+a4\n+a5\n+a6\n+a7\n+a8\n+a9\n+a10\n+a11\n+a12",
        ),
        ("b.rs", "@@ -0,0 +1 @@\n+b1"),
    ])));
    let theme = theme();
    let start = panel.render(&theme, 60, 12);
    assert!(start[1].contains("a.rs") && start[2].contains("b.rs"));
    panel.mouse(mouse(ScrollDown, 8));
    let scrolled = panel.render(&theme, 60, 12);
    assert_ne!(start, scrolled);
    assert!(matches!(
        panel.mouse(mouse(Up(MouseButton::Left), 8)),
        Action::None
    ));
    panel.mouse(mouse(ScrollUp, 8));
    panel.render(&theme, 60, 12);
    panel.mouse(mouse(Down(MouseButton::Left), 2));
    assert!(panel
        .render(&theme, 60, 12)
        .iter()
        .any(|row| row.contains("b1")));
}

#[test]
fn review_frame_bounds_untrusted_text_and_wraps_without_raw_patch_headers() {
    let mut panel = DiffPanel::new(&json!({}));
    panel.apply(Ok(review(&[(
        "a.rs",
        "@@ -1 +1 @@\n-界界界界界界\n+\x1b]52;c;bad\x07safe",
    )])));
    for width in [1, 20, 55] {
        for height in [1, 8, 30] {
            let theme = theme();
            let rows = panel.render(&theme, width, height);
            assert_eq!(rows.len(), height);
            assert!(rows[0].starts_with(&theme.bg_prefix("diffPaneBg")));
            assert!(rows
                .iter()
                .all(|row| e_diff::style::text::visible_width(row) <= width));
            let text = rows.join("\n");
            assert!(!text.contains("@@"));
            assert!(!text.contains("]52"));
        }
    }
}

/// The transcript form: every row of the document in order, styled, with no
/// viewport padding — and the same escape hygiene as the live pane.
#[test]
fn document_emits_the_whole_review_without_viewport_clipping() {
    let mut panel = DiffPanel::new(&json!({}));
    panel.apply(Ok(review(&[
        ("a.rs", "@@ -0,0 +1,12 @@\n+a1\n+a2"),
        ("b.rs", "@@ -0,0 +1 @@\n+b1"),
    ])));
    let rows = panel.document(&theme(), 60);
    assert!(rows[0].contains("2 files changed"));
    let text = rows.join("\n");
    assert!(text.contains("a1") && text.contains("b1"));
    assert!(!text.contains("@@"));
    // No scroll state can hide a row from the document form.
    panel.apply(Ok(review(&[(
        "c.rs",
        "@@ -0,0 +1,50 @@\n+c1\n+c2\n+c3\n+c4\n+c5\n+c6\n+c7\n+c8\n+c9\n+c10\n+c11\n+c12\n+c13\n+c14\n+c15\n+c16\n+c17\n+c18\n+c19\n+c20\n+c21\n+c22\n+c23\n+c24\n+c25\n+c26\n+c27\n+c28\n+c29\n+c30\n+c31\n+c32\n+c33\n+c34\n+c35\n+c36\n+c37\n+c38\n+c39\n+c40\n+c41\n+c42\n+c43\n+c44\n+c45\n+c46\n+c47\n+c48\n+c49\n+c50",
    )])));
    let rows = panel.document(&theme(), 60);
    assert!(rows.iter().any(|row| row.contains("c50")));
}
