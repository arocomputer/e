//! Release bodies must contain only the requested version's authored notes.

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// Run the workflow's extractor against an in-memory changelog.
fn extract(tag: &str, changelog: &str) -> Output {
    let mut child = Command::new("sh")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/release-notes.sh"
        ))
        .arg(tag)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(changelog.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn release_body_keeps_summary_and_groups_without_neighboring_versions() {
    let body = "\nSeptember 9, 2026\n\n### A release title\n\nA short introduction.\n\n### New features\n\n- A feature.\n\n### Improvements\n\n- An improvement.\n\n### Fixes\n\n- A fix.\n\n";
    let changelog = format!(
        "# e\n\n## Unreleased\n\n- Not shipped.\n\n## 1.2.30\n\n- Not this version.\n\n## 1.2.3\n{body}## 1.2.2\n\n- Older.\n"
    );
    let output = extract("v1.2.3", &changelog);
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), body);
}

#[test]
fn unusable_release_sections_fail_without_emitting_partial_notes() {
    for changelog in [
        "## Unreleased\n\n- Not shipped.\n",
        "## 1.2.30\n\n- A different version.\n",
        "## 1.2.3\n\n \t\n",
        "## 1.2.3\n- First.\n## 1.2.3\n- Duplicate.\n",
    ] {
        let output = extract("v1.2.3", changelog);
        assert!(!output.status.success(), "accepted {changelog:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("expected one nonempty section"));
    }
}

#[test]
fn release_extraction_rejects_nonrelease_tags() {
    for tag in ["Unreleased", "1.2.3", "v1.2.3-rc.1", "v01.2.3", "v1x2x3"] {
        let output = extract(tag, "");
        assert!(!output.status.success(), "accepted {tag:?}");
        assert!(output.stdout.is_empty());
    }
}
