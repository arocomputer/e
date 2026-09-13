//! The review scanner's contract: correct snapshots, and no path by which a
//! repository can execute code or leak file contents through `/diff`.
use e_diff::diff;
use std::path::Path;
use std::process::Command;

/// Git fixture with local-only identity, no template hooks, and no signing.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "--template="]);
    git(dir.path(), &["config", "user.name", "Test"]);
    git(
        dir.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
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

#[tokio::test]
async fn workspace_diff_combines_staged_unstaged_and_untracked_without_writing_git() {
    let dir = repo();
    let root = dir.path();
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

#[tokio::test]
async fn unborn_repo_includes_staged_files_and_ignores_ignored_untracked_files() {
    let dir = repo();
    std::fs::write(dir.path().join("first.rs"), "hello\n").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "ignored\n").unwrap();
    std::fs::write(dir.path().join("ignored"), "do not display").unwrap();
    git(dir.path(), &["add", "."]);
    let snapshot = diff::load(dir.path(), Some(Path::new("first.rs")))
        .await
        .unwrap();
    assert_eq!(snapshot.files.len(), 2);
    assert!(snapshot.files.iter().all(|f| f.new));
    assert_eq!(snapshot.patch, "@@ -0,0 +1,1 @@\n+hello\n");
}

#[tokio::test]
async fn unusual_paths_deletions_and_binary_files_survive_git_parsing() {
    let dir = repo();
    let strange = "tab\tline\n界.rs";
    std::fs::write(dir.path().join(strange), "before\n").unwrap();
    std::fs::write(dir.path().join("deleted"), "gone\n").unwrap();
    std::fs::write(dir.path().join("binary"), b"a\0b").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "base"]);
    std::fs::write(dir.path().join(strange), "after\n").unwrap();
    std::fs::remove_file(dir.path().join("deleted")).unwrap();
    std::fs::write(dir.path().join("binary"), b"b\0c").unwrap();
    let snapshot = diff::load(dir.path(), Some(Path::new(strange)))
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

#[tokio::test]
async fn new_symlinks_preview_the_link_not_the_target_and_diff_helpers_do_not_run() {
    let dir = repo();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("target");
    std::fs::write(&secret, "private target text").unwrap();
    std::os::unix::fs::symlink(&secret, dir.path().join("link")).unwrap();
    std::fs::write(dir.path().join("tracked"), "before\n").unwrap();
    git(dir.path(), &["add", "tracked"]);
    git(dir.path(), &["commit", "-m", "base"]);
    git(dir.path(), &["config", "diff.external", "false"]);
    std::fs::write(
        dir.path().join(".gitattributes"),
        "tracked filter=probe diff=probe\n",
    )
    .unwrap();
    for key in [
        "filter.probe.clean",
        "filter.probe.process",
        "diff.probe.textconv",
    ] {
        git(dir.path(), &["config", key, "touch .git/filter-ran; cat"]);
    }
    git(dir.path(), &["config", "filter.probe.required", "true"]);
    std::fs::write(dir.path().join("tracked"), "after\n").unwrap();
    let snapshot = diff::load(dir.path(), Some(Path::new("link")))
        .await
        .unwrap();
    assert!(snapshot.patch.contains(&secret.display().to_string()));
    assert!(!snapshot.patch.contains("private target text"));
    let tracked = diff::load(dir.path(), Some(Path::new("tracked")))
        .await
        .unwrap();
    assert!(
        tracked.patch.contains("+after"),
        "external diff helper must be disabled"
    );
    assert!(
        !dir.path().join(".git/filter-ran").exists(),
        "preview must not execute Git filters"
    );
}

#[tokio::test]
async fn non_repository_returns_an_explanation() {
    let dir = tempfile::tempdir().unwrap();
    assert!(diff::load(dir.path(), None)
        .await
        .unwrap_err()
        .contains("Git working tree"));
}

#[tokio::test]
async fn continuous_review_includes_tracked_and_untracked_files_without_index_writes() {
    let dir = repo();
    std::fs::write(dir.path().join("tracked"), "before\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "base"]);
    let index = std::fs::read(dir.path().join(".git/index")).unwrap();
    std::fs::write(dir.path().join("tracked"), "after\n").unwrap();
    std::fs::write(dir.path().join("untracked"), "new\n").unwrap();
    let review = diff::load_review(dir.path()).await.unwrap();
    assert_eq!(review.patches.len(), 2);
    assert!(review.patches[0].1.contains("+after"));
    assert!(review.patches[1].1.contains("+new"));
    assert!(!review.truncated);
    assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index);
}

/// The trust boundary review #2 asked for: an executable named `git` that
/// lives inside the workspace, or resolves through a relative PATH entry,
/// is never the program the scanner runs.
#[test]
fn resolve_git_refuses_workspace_and_relative_path_candidates() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace = workspace.path().canonicalize().unwrap();
    let rogue = workspace.join("git");
    std::fs::write(&rogue, b"#!/bin/sh\necho rogue\n").unwrap();
    std::fs::set_permissions(&rogue, {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o755)
    })
    .unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside = outside.path().canonicalize().unwrap();
    let good = outside.join("git");
    std::fs::write(&good, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&good, {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o755)
    })
    .unwrap();

    // A relative entry (the `.` in `PATH=.:...`) never contributes.
    let path = std::ffi::OsString::from(format!(".:{}", outside.display()));
    let resolved = diff::resolve_git(Some(&path), &workspace).unwrap();
    assert!(resolved.starts_with(&outside));

    // A `git` inside the workspace is skipped even under an absolute entry.
    let path = std::ffi::OsString::from(workspace.display().to_string());
    let resolved = diff::resolve_git(Some(&path), &workspace);
    assert!(resolved.is_none() || !resolved.unwrap().starts_with(&workspace));
}

/// Review #4's guarantee, pinned directly: every component between the root
/// and the file must be real — a symlinked intermediate directory earns an
/// error, not the target's contents.
#[test]
fn open_in_root_rejects_symlinks_in_every_component() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "private").unwrap();
    std::fs::create_dir(root.path().join("sub")).unwrap();
    std::fs::write(root.path().join("sub/file"), "plain").unwrap();

    let mut opened = diff::open_in_root(root.path(), Path::new("sub/file")).unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut opened, &mut text).unwrap();
    assert_eq!(text, "plain");

    std::fs::remove_file(root.path().join("sub/file")).unwrap();
    std::fs::remove_dir(root.path().join("sub")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("sub")).unwrap();
    let leak = diff::open_in_root(root.path(), Path::new("sub/secret"));
    assert!(
        leak.is_err(),
        "an intermediate symlink must not be followed"
    );

    assert!(diff::open_in_root(root.path(), Path::new("../escape")).is_err());
    assert!(diff::open_in_root(root.path(), Path::new("/etc/passwd")).is_err());
}
