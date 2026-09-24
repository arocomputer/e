//! Collapsed paste labels own their payload only while their draft range exists.
mod common;

use common::{env_lock, Home};
use ulo::tui::composer::{Editor, EditorResult, Key};

/// Submit through the same editor action used by the TUI.
fn submit(editor: &mut Editor) -> String {
    let EditorResult::Submit(text) = editor.key(Key::Enter) else {
        panic!("expected submission");
    };
    text
}

#[test]
fn labels_count_unicode_characters_and_normalize_newlines() {
    let _lock = env_lock();
    let _home = Home::new("paste-counts");
    let mut editor = Editor::new();
    let html = format!("<div>{}</div>", "界".repeat(1200));
    editor.insert_paste(&html);
    assert_eq!(editor.text(), "[Pasted text #1, 1211 chars]");
    editor.render(&ulo::tui::theme::resolve("dark", false), 20, 10);
    assert_eq!(editor.text(), "[Pasted text #1, 1211 chars]");
    assert_eq!(submit(&mut editor), html);

    let text = format!("a\r\nb\rc\n\n{}\n", "界".repeat(1200));
    editor.insert_paste(&text);
    assert_eq!(editor.text(), "[Pasted text #1, 1208 chars]");
    assert_eq!(
        submit(&mut editor),
        text.replace("\r\n", "\n").replace('\r', "\n")
    );
}

#[test]
fn paste_labels_use_image_attachment_grey_without_tinting_the_prompt() {
    let _lock = env_lock();
    let _home = Home::new("paste-colour");
    let theme = ulo::tui::theme::resolve("dark", false);
    let mut editor = Editor::new();
    editor.insert_paste(&"x".repeat(1200));
    let label = editor.text();
    editor.insert_str(" explain");
    let rows = editor.render(&theme, 100, 10);
    assert!(rows[1].contains(&format!("{} explain", theme.fg("dim", &label))));
}

#[test]
fn paste_numbers_belong_to_the_draft_and_restart_when_no_attachments_remain() {
    let _lock = env_lock();
    let _home = Home::new("paste-numbers");
    let mut editor = Editor::new();
    let payload = "x".repeat(1200);
    editor.insert_paste("short paste ");
    editor.insert_paste(&payload);
    let first = "[Pasted text #1, 1200 chars]";
    assert_eq!(editor.text(), format!("short paste {first}"));
    editor.insert_paste(&payload);
    assert!(editor.text().ends_with("[Pasted text #2, 1200 chars]"));
    assert_eq!(
        submit(&mut editor),
        format!("short paste {payload}{payload}")
    );

    editor.insert_paste(&payload);
    assert_eq!(editor.text(), first);
    editor.set_text("");
    editor.insert_paste(&payload);
    assert_eq!(editor.text(), first);
    editor.key(Key::Backspace);
    editor.insert_paste(&payload);
    assert_eq!(editor.text(), first);
}

#[test]
fn deleting_or_replacing_any_part_of_a_marker_retires_the_whole_payload() {
    let _lock = env_lock();
    let _home = Home::new("paste-delete");
    let cases: &[&[Key]] = &[
        &[Key::Backspace],
        &[Key::Home, Key::Delete],
        &[Key::KillWord],
        &[Key::KillToStart],
        &[Key::Home, Key::KillToEnd],
        &[Key::SelectLeft, Key::Backspace],
        &[Key::Home, Key::SelectRight, Key::Delete],
        &[Key::Left, Key::KillToEnd],
        &[Key::Left, Key::Char('!')],
        &[Key::SelectLeft, Key::Char('!')],
    ];
    for (case, keys) in cases.iter().enumerate() {
        let mut editor = Editor::new();
        editor.insert_paste(&"secret payload".repeat(100));
        let marker = editor.text();
        for key in *keys {
            editor.key(*key);
        }
        let expected = if matches!(keys.last(), Some(Key::Char('!'))) {
            "!"
        } else {
            ""
        };
        assert_eq!(editor.text(), expected, "case {case}");
        editor.insert_str(&marker);
        assert_eq!(
            submit(&mut editor),
            format!("{expected}{marker}"),
            "case {case}"
        );
    }
}

#[test]
fn edits_shift_owned_ranges_without_touching_neighboring_pastes() {
    let _lock = env_lock();
    let _home = Home::new("paste-ranges");
    let mut editor = Editor::new();
    let first = "a".repeat(1200);
    let second = "b".repeat(1200);
    editor.insert_paste(&first);
    editor.insert_str(" between ");
    editor.insert_paste(&second);
    editor.key(Key::Home);
    editor.insert_str("界 ");
    editor.key(Key::Delete);
    assert_eq!(editor.expanded_text(), format!("界  between {second}"));
    editor.key(Key::End);
    editor.insert_str(" end");
    assert_eq!(submit(&mut editor), format!("界  between {second} end"));
}

#[test]
fn literal_markers_in_typed_text_or_payloads_never_expand() {
    let _lock = env_lock();
    let _home = Home::new("paste-literals");
    let mut editor = Editor::new();
    let marker = "[Pasted text #2, 1200 chars]";
    let first = format!("{}{marker}", "a".repeat(1200));
    let second = "b".repeat(1200);
    editor.insert_paste(&first);
    editor.insert_paste(&second);
    editor.insert_str(marker);
    assert_eq!(submit(&mut editor), format!("{first}{second}{marker}"));
}

#[test]
fn history_navigation_preserves_draft_pastes_but_clear_discards_saved_payloads() {
    let _lock = env_lock();
    let _home = Home::new("paste-history");
    let mut editor = Editor::new();
    editor.push_history("older".into());
    let payload = "x".repeat(1200);
    editor.insert_paste(&payload);
    let marker = editor.text();
    editor.key(Key::Up);
    assert_eq!(editor.expanded_text(), "older");
    editor.key(Key::Down);
    assert_eq!(editor.text(), marker);
    assert_eq!(editor.expanded_text(), payload);

    editor.key(Key::Up);
    editor.set_text("");
    editor.key(Key::Down);
    assert_eq!(editor.expanded_text(), "");
    editor.insert_str(&marker);
    assert_eq!(submit(&mut editor), marker);
}

#[test]
fn completion_keeps_prefix_attachments_and_replacing_a_draft_discards_them() {
    let _lock = env_lock();
    let _home = Home::new("paste-completion");
    let mut editor = Editor::new();
    let payload = "x".repeat(1200);
    editor.insert_str("界 ");
    editor.insert_paste(&payload);
    let prefix = editor.text();
    editor.insert_str(" @fo");
    editor.replace_suffix(prefix.chars().count() + 1, "foo.rs");
    assert_eq!(editor.text(), format!("{prefix} foo.rs"));
    assert_eq!(editor.expanded_text(), format!("界 {payload} foo.rs"));
    let literal = editor.text();
    editor.set_text(&literal);
    assert_eq!(submit(&mut editor), literal);
}

#[test]
fn label_override_does_not_determine_attachment_identity() {
    let _lock = env_lock();
    let home = Home::new("paste-label");
    home.write("settings.json", r#"{"paste_label":"[attachment]"}"#);
    let mut editor = Editor::new();
    let first = "a".repeat(1200);
    let second = "b".repeat(1200);
    editor.insert_paste(&first);
    editor.insert_paste(&second);
    assert_eq!(editor.text(), "[attachment][attachment]");
    editor.key(Key::Home);
    editor.key(Key::Delete);
    assert_eq!(submit(&mut editor), second);
}
