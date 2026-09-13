//! Workspace diff reads and the split panel's navigation/attachment contract.
mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use e::core::diff::{self, File, Snapshot};
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

/// Two files with enough context to test navigation and immutable selections.
fn snapshot(selected: &str, patch: &str) -> Snapshot {
    Snapshot {
        files: ["a.rs", "b.rs"]
            .iter()
            .map(|path| File {
                path: path.into(),
                added: Some(1),
                removed: Some(1),
                new: false,
            })
            .collect(),
        selected: Some(selected.into()),
        patch: patch.into(),
    }
}

#[test]
fn diff_selection_attaches_a_snapshot_and_remains_deletable() {
    let _lock = common::env_lock();
    let _home = common::Home::new("diff-attachment");
    let mut panel = DiffPanel::new(1);
    panel.apply(None, Ok(snapshot("a.rs", "@@ -1 +1 @@\n-before\n+after")));
    panel.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    panel.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    panel.key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    let Action::Attach { label, content } =
        panel.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("expected selected diff");
    };
    assert!(content.ends_with("-before\n+after"));
    assert!(content.contains("@@ -1 +1 @@"));
    assert!(!panel.focused);
    panel.apply(
        Some("a.rs".into()),
        Ok(snapshot("a.rs", "@@ -1 +1 @@\n-before\n+later")),
    );
    assert!(!content.contains("later"));
    let mut editor = e::tui::composer::Editor::new();
    editor.insert_attachment(&label, &content);
    assert_eq!(editor.expanded_text(), content);
    editor.key(e::tui::composer::Key::Backspace);
    assert_eq!(editor.expanded_text(), "");
}

#[test]
fn mouse_selection_waits_for_enter_and_refresh_discards_changed_selection() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let _lock = common::env_lock();
    let _home = common::Home::new("diff-mouse");
    let mut panel = DiffPanel::new(1);
    panel.apply(None, Ok(snapshot("a.rs", "@@ -1 +1 @@\n-before\n+after")));
    let theme = e::tui::theme::resolve("dark", false);
    let rows = panel.render(&theme, 60, 30);
    let row = rows.iter().position(|row| row.contains("-before")).unwrap() as u16;
    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), row),
        (MouseEventKind::Drag(MouseButton::Left), row + 1),
        (MouseEventKind::Up(MouseButton::Left), row + 1),
    ] {
        assert!(matches!(
            panel.mouse(MouseEvent {
                kind,
                column: 1,
                row,
                modifiers: KeyModifiers::NONE
            }),
            Action::None
        ));
    }
    assert!(panel.focused);
    assert!(panel.render(&theme, 60, 30).join("\n").contains("\x1b[7m"));
    panel.apply(
        Some("a.rs".into()),
        Ok(snapshot("a.rs", "@@ -1 +1 @@\n-before\n+later")),
    );
    assert!(!panel.render(&theme, 60, 30).join("\n").contains("\x1b[7m"));
    let Action::Attach { content, .. } =
        panel.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("expected current line attachment");
    };
    assert!(content.ends_with("+later"));
    assert!(!content.contains("-before"));
}

#[test]
fn late_refresh_cannot_change_the_file_the_user_selected() {
    let _lock = common::env_lock();
    let _home = common::Home::new("diff-refresh");
    let mut panel = DiffPanel::new(1);
    panel.apply(None, Ok(snapshot("a.rs", "@@ -1 +1 @@\n-a\n+b")));
    panel.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(panel.selected.as_deref(), Some(Path::new("b.rs")));
    panel.apply(Some("a.rs".into()), Ok(snapshot("a.rs", "old response")));
    assert_eq!(panel.selected.as_deref(), Some(Path::new("b.rs")));
    assert!(panel.dirty);
    assert!(!panel.loading);
}

#[test]
fn diff_frame_uses_shared_dividers_and_bounds_untrusted_text() {
    let _lock = common::env_lock();
    let _home = common::Home::new("diff-frame");
    let mut panel = DiffPanel::new(1);
    panel.apply(
        None,
        Ok(snapshot(
            "a.rs",
            "@@ -1 +1 @@\n-界界界界界界\n+\x1b]52;c;bad\x07safe",
        )),
    );
    for width in [1, 20, 55] {
        for height in [1, 8, 30] {
            let theme = e::tui::theme::resolve("dark", false);
            let rows = panel.render(&theme, width, height);
            assert_eq!(rows.len(), height);
            assert_eq!(rows[0], theme.fg("border", &"─".repeat(width)));
            assert!(
                rows.iter()
                    .all(|r| e::tui::markdown::visible_width(r) <= width),
                "{width}x{height}: {rows:?}"
            );
            assert!(!rows.join("\n").contains("]52"));
        }
    }
}
