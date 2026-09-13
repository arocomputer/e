//! Workspace diff reads and the split panel's navigation/attachment contract.
mod common;

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use e::core::diff::{self, File, Review};
use e::tui::diffpanel::{Action, DiffPanel};
use std::path::Path;
use std::process::Command;

/// Git fixture with local-only identity, no template hooks, and no signing.
fn repo() -> common::Home {
    let dir = common::Home::new("diff-repo");
    git(dir.dir.as_path(), &["init", "--template="]);
    git(dir.dir.as_path(), &["config", "user.name", "Test"]);
    git(
        dir.dir.as_path(),
        &["config", "user.email", "test@example.invalid"],
    );
    git(dir.dir.as_path(), &["config", "commit.gpgsign", "false"]);
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn workspace_diff_combines_staged_unstaged_and_untracked_without_writing_git() {
    let _lock = common::env_lock();
    let dir = repo();
    let root = dir.dir.as_path();
    std::fs::write(root.join("tracked.rs"), "one\ntwo\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    std::fs::write(root.join("tracked.rs"), "one\nstaged\n").unwrap();
    git(root, &["add", "."]);
    std::fs::write(root.join("tracked.rs"), "one\nworking\n").unwrap();
    std::fs::write(root.join("new file.rs"), "new\n").unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let snapshot = diff::load(root, Some(Path::new("tracked.rs")))
        .await
        .unwrap();
    assert_eq!(snapshot.files.len(), 2);
    assert!(snapshot.patch.contains("-two\n+working"));
    assert!(!snapshot.patch.contains("+staged"));
    assert_eq!(snapshot.files[1].added, Some(1));
    assert_eq!(snapshot.files[1].removed, Some(1));
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    let next = diff::load(root, Some(Path::new("new file.rs")))
        .await
        .unwrap();
    assert!(next.patch.contains("+new"));
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn unborn_repo_includes_staged_files_and_ignores_ignored_untracked_files() {
    let _lock = common::env_lock();
    let dir = repo();
    std::fs::write(dir.dir.as_path().join("first.rs"), "hello\n").unwrap();
    std::fs::write(dir.dir.as_path().join(".gitignore"), "ignored\n").unwrap();
    std::fs::write(dir.dir.as_path().join("ignored"), "do not display").unwrap();
    git(dir.dir.as_path(), &["add", "."]);
    let snapshot = diff::load(dir.dir.as_path(), Some(Path::new("first.rs")))
        .await
        .unwrap();
    assert_eq!(snapshot.files.len(), 2);
    assert!(snapshot.files.iter().all(|f| f.new));
    assert_eq!(snapshot.patch, "@@ -0,0 +1,1 @@\n+hello\n");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn unusual_paths_deletions_and_binary_files_survive_git_parsing() {
    let _lock = common::env_lock();
    let dir = repo();
    let strange = "tab\tline\n界.rs";
    std::fs::write(dir.dir.as_path().join(strange), "before\n").unwrap();
    std::fs::write(dir.dir.as_path().join("deleted"), "gone\n").unwrap();
    std::fs::write(dir.dir.as_path().join("binary"), b"a\0b").unwrap();
    git(dir.dir.as_path(), &["add", "."]);
    git(dir.dir.as_path(), &["commit", "-m", "base"]);
    std::fs::write(dir.dir.as_path().join(strange), "after\n").unwrap();
    std::fs::remove_file(dir.dir.as_path().join("deleted")).unwrap();
    std::fs::write(dir.dir.as_path().join("binary"), b"b\0c").unwrap();
    let snapshot = diff::load(dir.dir.as_path(), Some(Path::new(strange)))
        .await
        .unwrap();
    assert_eq!(snapshot.files.len(), 3);
    assert!(snapshot.files.iter().any(|f| f.path == Path::new(strange)));
    assert!(snapshot
        .files
        .iter()
        .any(|f| f.path == Path::new("deleted") && f.removed == Some(1)));
    assert!(snapshot
        .files
        .iter()
        .any(|f| f.path == Path::new("binary") && f.added.is_none()));
    assert!(snapshot.patch.contains("+after"));
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn new_symlinks_preview_the_link_not_the_target_and_diff_helpers_do_not_run() {
    let _lock = common::env_lock();
    let dir = repo();
    let outside_home = common::Home::new("diff-outside");
    let outside = outside_home.dir.join("target");
    std::fs::write(&outside, "private target text").unwrap();
    std::os::unix::fs::symlink(&outside, dir.dir.as_path().join("link")).unwrap();
    std::fs::write(dir.dir.as_path().join("tracked"), "before\n").unwrap();
    git(dir.dir.as_path(), &["add", "tracked"]);
    git(dir.dir.as_path(), &["commit", "-m", "base"]);
    git(dir.dir.as_path(), &["config", "diff.external", "false"]);
    std::fs::write(
        dir.dir.join(".gitattributes"),
        "tracked filter=probe diff=probe\n",
    )
    .unwrap();
    for key in [
        "filter.probe.clean",
        "filter.probe.process",
        "diff.probe.textconv",
    ] {
        git(
            dir.dir.as_path(),
            &["config", key, "touch .git/filter-ran; cat"],
        );
    }
    git(
        dir.dir.as_path(),
        &["config", "filter.probe.required", "true"],
    );
    std::fs::write(dir.dir.as_path().join("tracked"), "after\n").unwrap();
    let snapshot = diff::load(dir.dir.as_path(), Some(Path::new("link")))
        .await
        .unwrap();
    assert!(snapshot.patch.contains(&outside.display().to_string()));
    assert!(!snapshot.patch.contains("private target text"));
    let tracked = diff::load(dir.dir.as_path(), Some(Path::new("tracked")))
        .await
        .unwrap();
    assert!(
        tracked.patch.contains("+after"),
        "external diff helper must be disabled"
    );
    assert!(
        !dir.dir.join(".git/filter-ran").exists(),
        "preview must not execute Git filters"
    );
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn non_repository_returns_an_explanation() {
    let _lock = common::env_lock();
    let dir = common::Home::new("diff-not-repo");
    assert!(diff::load(dir.dir.as_path(), None)
        .await
        .unwrap_err()
        .contains("Git working tree"));
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
    let _lock = common::env_lock();
    let _home = common::Home::new("diff-source");
    let mut panel = DiffPanel::new(1);
    let source =
        "@@ -0,0 +1 @@\n+\tconst 界 = 'a long line that wraps across several display rows';";
    panel.apply(Ok(review(&[("src/lib/time.ts", source)])));
    let theme = e::tui::theme::resolve("dark", false);
    let rows = panel.render(&theme, 35, 30);
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
    let mut editor = e::tui::composer::Editor::new();
    editor.insert_attachment(&label, &content);
    panel.apply(Ok(review(&[("src/lib/time.ts", "@@ -0,0 +1 @@\n+later")])));
    assert_eq!(editor.expanded_text(), content);
    editor.key(e::tui::composer::Key::Backspace);
    assert_eq!(editor.expanded_text(), content);
    editor.key(e::tui::composer::Key::Backspace);
    assert!(editor.expanded_text().is_empty());
}

#[test]
fn refresh_during_a_drag_cannot_attach_different_source() {
    use MouseEventKind::*;
    let _lock = common::env_lock();
    let _home = common::Home::new("diff-refresh");
    let mut panel = DiffPanel::new(1);
    panel.apply(Ok(review(&[("a.rs", "@@ -1 +1 @@\n-before\n+after")])));
    let rows = panel.render(&e::tui::theme::resolve("dark", false), 60, 30);
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
    let _lock = common::env_lock();
    let _home = common::Home::new("diff-scroll");
    let mut panel = DiffPanel::new(1);
    panel.apply(Ok(review(&[
        (
            "a.rs",
            "@@ -0,0 +1,12 @@\n+a1\n+a2\n+a3\n+a4\n+a5\n+a6\n+a7\n+a8\n+a9\n+a10\n+a11\n+a12",
        ),
        ("b.rs", "@@ -0,0 +1 @@\n+b1"),
    ])));
    let theme = e::tui::theme::resolve("dark", false);
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
    let _lock = common::env_lock();
    let _home = common::Home::new("diff-frame");
    let mut panel = DiffPanel::new(1);
    panel.apply(Ok(review(&[(
        "a.rs",
        "@@ -1 +1 @@\n-界界界界界界\n+\x1b]52;c;bad\x07safe",
    )])));
    for width in [1, 20, 55] {
        for height in [1, 8, 30] {
            let theme = e::tui::theme::resolve("dark", false);
            let rows = panel.render(&theme, width, height);
            assert_eq!(rows.len(), height);
            assert!(rows[0].starts_with(&theme.bg_prefix("diffPaneBg")));
            assert!(rows
                .iter()
                .all(|row| e::tui::markdown::visible_width(row) <= width));
            assert!(!rows.join("\n").contains("@@"));
            assert!(!rows.join("\n").contains("]52"));
        }
    }
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn continuous_review_includes_tracked_and_untracked_files_without_index_writes() {
    let _lock = common::env_lock();
    let dir = repo();
    std::fs::write(dir.dir.join("tracked"), "before\n").unwrap();
    git(&dir.dir, &["add", "."]);
    git(&dir.dir, &["commit", "-m", "base"]);
    let index = std::fs::read(dir.dir.join(".git/index")).unwrap();
    std::fs::write(dir.dir.join("tracked"), "after\n").unwrap();
    std::fs::write(dir.dir.join("untracked"), "new\n").unwrap();
    let review = diff::load_review(&dir.dir).await.unwrap();
    assert_eq!(review.patches.len(), 2);
    assert!(review.patches[0].1.contains("+after"));
    assert!(review.patches[1].1.contains("+new"));
    assert!(!review.truncated);
    assert_eq!(std::fs::read(dir.dir.join(".git/index")).unwrap(), index);
}
