//! Prompt history: appended per prompt, read back newest-last and bounded,
//! private to the user, and trimmed once it grows past twice the recall.

mod common;

use common::{env_lock, Home};
use e::tui::history;

#[test]
fn prompts_append_in_order_and_load_bounded() {
    let _lock = env_lock();
    let home = Home::new("history");
    assert!(history::load(10).is_empty(), "no file is an empty history");
    history::append("first");
    history::append("second\nline");
    history::append("   ");
    history::append("second\nline");
    assert_eq!(history::load(10), vec!["first", "second\nline"]);
    assert_eq!(history::load(1), vec!["second\nline"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.dir.join("history.jsonl"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // A corrupt line is skipped, not fatal.
    std::fs::write(
        home.dir.join("history.jsonl"),
        "\"kept\"\nnot json\n\"also kept\"\n",
    )
    .unwrap();
    assert_eq!(history::load(10), vec!["kept", "also kept"]);
}

#[test]
fn the_file_is_trimmed_to_the_recall_window_once_it_doubles() {
    let _lock = env_lock();
    let home = Home::new("history-trim");
    for i in 0..(2 * history::RECALL + 1) {
        history::append(&format!("prompt {i}"));
    }
    let lines = std::fs::read_to_string(home.dir.join("history.jsonl"))
        .unwrap()
        .lines()
        .count();
    assert_eq!(lines, history::RECALL);
    let recalled = history::load(history::RECALL);
    assert_eq!(recalled.len(), history::RECALL);
    assert_eq!(
        recalled.last().unwrap(),
        &format!("prompt {}", 2 * history::RECALL)
    );
}
