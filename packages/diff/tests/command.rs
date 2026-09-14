//! The command's `show` object: a titled unified diff the host paints, with
//! headers added for patches that lack them, and a text block when clean.

use std::path::PathBuf;

use e_diff::diff::{File, Review};

#[test]
fn a_review_becomes_a_titled_unified_diff() {
    let review = Review {
        files: vec![
            File { path: PathBuf::from("src/a.rs"), added: Some(3), removed: Some(1), new: false },
            File { path: PathBuf::from("new.txt"), added: Some(1), removed: Some(0), new: true },
        ],
        patches: vec![
            (
                PathBuf::from("src/a.rs"),
                "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1,3 @@\n-x\n+y\n+z\n+w\n".into(),
            ),
            (PathBuf::from("new.txt"), "@@ -0,0 +1,1 @@\n+hello\n".into()),
        ],
        truncated: true,
    };
    let shown = e_diff::command::show(&review);
    assert_eq!(shown["title"], "2 files changed +4 -1 (truncated)");
    assert_eq!(shown["format"], "diff");
    let body = shown["body"].as_str().unwrap();
    assert!(body.contains("+++ b/src/a.rs\n@@ -1 +1,3 @@"));
    assert!(
        body.contains("--- a/new.txt\n+++ b/new.txt\n@@ -0,0 +1,1 @@\n+hello\n"),
        "{body}"
    );

    let clean = e_diff::command::show(&Review {
        files: vec![],
        patches: vec![],
        truncated: false,
    });
    assert_eq!(clean["format"], "text");
}
