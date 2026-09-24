//! App behavior regressions using the same state and handlers as the terminal loop.
/// Interrupt dismisses transient navigation without trusting or submitting.
#[test]
fn interrupt_dismisses_trust_and_queue_review_and_drops_held_prompt() {
    let mut app = session_app();
    app.trust = Some(crate::trustpanel::TrustStage::new(&app.agent.cwd()));
    app.queue_review = Some(QueueReview {
        entries: vec![(1, "queued".into())],
        dirty: vec![false],
        selected: 0,
        visible: true,
    });
    app.pending_initial = Some("must not run".into());
    app.editor.set_text("draft");
    app.interrupt_or_exit();
    assert!(app.trust.is_none());
    assert!(app.queue_review.is_none());
    assert!(app.pending_initial.is_none());
    assert!(app.editor.is_empty());
    assert!(!app.agent.is_streaming());
    assert!(!app.should_quit);
}

use super::*;

/// Mouse navigation pauses output following without recalling or editing prompts.
#[test]
fn main_wheel_keeps_the_draft_and_reading_position_through_output_and_review() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    let mut app = session_app();
    app.editor.push_history("old prompt".into());
    app.editor.set_text("unfinished draft");
    app.transcript.push(Block::new(
        Kind::Assistant,
        (0..80)
            .map(|i| format!("row {i:02}\n\n"))
            .collect::<String>(),
    ));
    app.mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 1,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        80,
        24,
    );
    let position = app
        .conversation_scroll
        .expect("wheel must scroll normal chat");
    let first = app.frame(80, 24)[0].clone();
    app.transcript
        .push(Block::new(Kind::Assistant, "new output"));
    assert_eq!(app.frame(80, 24)[0], first);
    assert_eq!(app.editor.text(), "unfinished draft");
    app.viewer = Some(Viewer::new());
    app.viewer_frame(80, 24);
    app.viewer_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 80, 24);
    assert_eq!(app.conversation_scroll, Some(position));
    let resized = app.frame(60, 18);
    assert_eq!(resized.len(), 18);
    assert!(resized[15].contains("unfinished draft"));
    assert!(app.conversation_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE), 60, 18));
    assert!(app.conversation_scroll.is_none());
    assert!(app
        .frame(60, 18)
        .iter()
        .any(|row| row.contains("new output")));
}

/// Collapsed thinking stays available to review and to the settings toggle.
#[test]
fn thinking_can_be_revealed_after_it_arrived_collapsed() {
    let home = std::env::temp_dir().join(format!("ulo-thinking-{}", uuid::Uuid::new_v4()));
    ulo_core::config::home::with_home(home.clone(), || {
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("settings.json"),
            include_str!("../../../cli/tests/fixtures/config/settings-v1-conversation.json"),
        )
        .unwrap();
        let mut app = session_app();
        app.refresh_status_cache();
        assert!(app.bottom_pinned);
        assert_eq!(app.scroll_lines, 5);
        assert_eq!(app.scroll_hint, "Reading earlier output");
        assert_eq!(app.tool_history_limit, 4);
        assert_eq!(app.tool_history_hint, "{count} successful calls in review");
        app.on_session_event(SessionEvent::TurnStart);
        app.on_session_event(SessionEvent::ReasoningDelta("retained thought".into()));
        app.end_thinking_burst();
        let main = app.transcript_frame(80).join("\n");
        assert!(main.contains("Reasoning available in review"));
        assert!(!main.contains("retained thought"));
        assert!(app
            .viewer_rows(80, false)
            .join("\n")
            .contains("retained thought"));
        ulo_core::config::settings::set_string("show_thinking", "on").unwrap();
        app.refresh_status_cache();
        assert!(app
            .transcript_frame(80)
            .join("\n")
            .contains("retained thought"));
        ulo_core::config::settings::set_string("show_thinking", "off").unwrap();
        app.refresh_status_cache();
        assert!(!app
            .transcript_frame(80)
            .join("\n")
            .contains("retained thought"));
        assert!(app
            .viewer_rows(80, false)
            .join("\n")
            .contains("retained thought"));
    });
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn tab_title_shortens_to_two_components() {
    assert_eq!(
        title_path_from(
            std::path::Path::new("/Volumes/v0/workspaces/worktrees/ulo/bold-fox"),
            ""
        ),
        "ulo/bold-fox"
    );
    assert_eq!(title_path_from(std::path::Path::new("/etc"), ""), "etc");
    assert_eq!(title_path_from(std::path::Path::new("/"), ""), "/");
}

#[test]
fn tab_title_is_home_relative_under_home() {
    assert_eq!(
        title_path_from(std::path::Path::new("/Users/fschr/code/x"), "/Users/fschr"),
        "~/code/x"
    );
    assert_eq!(
        title_path_from(std::path::Path::new("/Users/fschr"), "/Users/fschr"),
        "~"
    );
    assert_eq!(
        title_path_from(
            std::path::Path::new("/Users/fschr/code/a/b/c"),
            "/Users/fschr"
        ),
        "~/b/c"
    );
}

fn node(
    id: &str,
    parent: Option<&str>,
    message: ulo_core::providers::ChatMessage,
) -> ulo_core::session::Node {
    ulo_core::session::Node {
        id: id.into(),
        parent: parent.map(String::from),
        message,
    }
}

#[test]
fn tree_items_lists_user_turns_and_flags_branch_points() {
    use ulo_core::providers::ChatMessage;
    let nodes = vec![
        node("1", None, ChatMessage::user("root question")),
        node(
            "2",
            Some("1"),
            ChatMessage::assistant("reply A", Vec::new()),
        ),
        // A second child of "1": root was rewound and branched from once.
        node("3", Some("1"), ChatMessage::user("second try")),
    ];
    let items = tree_items(&nodes);
    // Only user-role nodes are offered as rewind points.
    assert_eq!(items.len(), 2);
    let (id, preview, branched) = &items[0];
    assert_eq!(id, "1");
    assert_eq!(preview, "root question");
    assert!(!branched, "the root itself has no parent to branch under");
    let (id, preview, branched) = &items[1];
    assert_eq!(id, "3");
    assert_eq!(preview, "second try");
    assert!(*branched, "\"1\" now has two children — a branch point");
}

#[test]
fn rewind_target_replays_ancestors_and_restores_the_chosen_prompt() {
    use ulo_core::providers::ChatMessage;
    let nodes = vec![
        node("1", None, ChatMessage::user("first")),
        node("2", Some("1"), ChatMessage::assistant("reply", Vec::new())),
        node("3", Some("2"), ChatMessage::user("second\nwith details")),
    ];
    let (head, messages, prompt) = rewind_target(&nodes, "3").expect("node 3 exists");
    assert_eq!(head.as_deref(), Some("2"), "rewinds to just before node 3");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "first");
    assert_eq!(messages[1].content, "reply");
    assert_eq!(prompt, "second\nwith details");
}

#[test]
fn rewind_target_to_the_root_yields_an_empty_history_and_no_head() {
    use ulo_core::providers::ChatMessage;
    let nodes = vec![node("1", None, ChatMessage::user("only message"))];
    let (head, messages, prompt) = rewind_target(&nodes, "1").expect("node 1 exists");
    assert!(head.is_none());
    assert!(messages.is_empty());
    assert_eq!(prompt, "only message");
}

#[test]
fn rewind_target_is_none_for_an_unknown_id() {
    use ulo_core::providers::ChatMessage;
    let nodes = vec![node("1", None, ChatMessage::user("only message"))];
    assert!(rewind_target(&nodes, "missing").is_none());
}

// ULO_HOME is process-global; serialize the tests below that set it.
static KEY_OF_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn key_of_matches_the_built_in_bindings_when_the_keymap_is_empty() {
    let keymap = crate::keybindings::Keymap::empty();
    let ctrl_w = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(matches!(key_of(&ctrl_w, &keymap), Some(Key::KillWord)));
    let plain_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    assert!(matches!(key_of(&plain_x, &keymap), Some(Key::Char('x'))));
    let command_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::SUPER);
    assert!(
        key_of(&command_v, &keymap).is_none(),
        "an unhandled Command key must not insert its printable character"
    );
    // On a non-empty composer ctrl+d is forward delete (the empty
    // composer's quit is the app-level handler's job, before key_of).
    let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert!(matches!(key_of(&ctrl_d, &keymap), Some(Key::Delete)));
}

#[test]
fn key_of_consults_an_override_before_the_built_in_binding() {
    let _guard = KEY_OF_ENV_LOCK
        .lock()
        .unwrap_or_else(|ulo| ulo.into_inner());
    let dir = std::env::temp_dir().join(format!(
        "ulo-key-of-override-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("ULO_HOME", &dir);
    std::fs::write(dir.join("keybindings.json"), r#"{"ctrl+w": "home"}"#).unwrap();

    let keymap = crate::keybindings::load();
    let ctrl_w = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(
        matches!(key_of(&ctrl_w, &keymap), Some(Key::Home)),
        "an override replaces the built-in action for that chord"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn key_of_none_override_swallows_a_built_in_chord() {
    let _guard = KEY_OF_ENV_LOCK
        .lock()
        .unwrap_or_else(|ulo| ulo.into_inner());
    let dir = std::env::temp_dir().join(format!(
        "ulo-key-of-none-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("ULO_HOME", &dir);
    // ctrl+j is a built-in binding for Newline; "none" must swallow it
    // rather than falling through to the default.
    std::fs::write(dir.join("keybindings.json"), r#"{"ctrl+j": "none"}"#).unwrap();

    let keymap = crate::keybindings::load();
    let ctrl_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
    assert!(key_of(&ctrl_j, &keymap).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reload_result_replaces_the_in_progress_notice() {
    let mut transcript = Transcript::default();
    let reload_block = transcript.push(Block::new(Kind::Notice, "reloading…"));
    transcript.push(Block::new(Kind::Notice, "an unrelated notice"));

    finish_reload_notice(&mut transcript, Some(reload_block));

    assert_eq!(transcript.blocks.len(), 2);
    assert_eq!(
        transcript.blocks[reload_block].text,
        "reloaded extensions, themes, and config — skills, prompts, and AGENTS.md are always read fresh"
    );
    assert_eq!(transcript.blocks[1].text, "an unrelated notice");
}

#[test]
fn initial_prompt_waits_for_first_visit_trust() {
    let mut pending = None;
    let now = stage_initial_prompt("inspect this repo".into(), true, &mut pending);
    assert!(now.is_none());
    assert_eq!(pending.as_deref(), Some("inspect this repo"));

    let mut pending = None;
    let now = stage_initial_prompt("inspect this repo".into(), false, &mut pending);
    assert_eq!(now.as_deref(), Some("inspect this repo"));
    assert!(pending.is_none());
}

#[test]
fn api_keys_bypass_input_hooks() {
    assert_eq!(input_route(true, true), InputRoute::ApiKey);
    assert_eq!(input_route(true, false), InputRoute::ApiKey);
    assert_eq!(input_route(false, true), InputRoute::Hook);
    assert_eq!(input_route(false, false), InputRoute::Direct);
}

#[test]
fn tab_title_prefers_the_session_name_over_the_path() {
    assert_eq!(tab_title("~/work", None), "ulo · ~/work");
    assert_eq!(
        tab_title("~/work", Some("fix the renderer")),
        "ulo · fix the renderer"
    );
    // A blank name falls back to the path, never an empty title.
    assert_eq!(tab_title("~/work", Some("   ")), "ulo · ~/work");
}

#[test]
fn input_hook_verdicts_apply_in_submission_order() {
    let mut pending = PendingInputVerdicts::default();
    let first = pending.reserve();
    let second = pending.reserve();

    let later = pending.complete(
        second,
        "second".into(),
        None,
        ulo_core::extensions::InputVerdict::default(),
    );
    assert!(later.is_empty(), "a later verdict must wait");

    let ordered = pending.complete(
        first,
        "first".into(),
        None,
        ulo_core::extensions::InputVerdict::default(),
    );
    assert_eq!(
        ordered
            .into_iter()
            .map(|(text, _, _)| text)
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
}

/// The initial launch prompt (`-i image.png "..."`) carries images
/// through the same hook-ordering machinery as plain text so an input
/// hook still sees it (the fix for the reported hook bypass) — this
/// pins that the images stay attached to the right sequence entry, not
/// dropped or swapped, once a hook actually sits in front of it.
#[test]
fn input_hook_verdicts_carry_images_with_the_right_sequence_entry() {
    let mut pending = PendingInputVerdicts::default();
    let text_only = pending.reserve();
    let with_images = pending.reserve();

    let image = ulo_core::providers::ImageInput {
        media_type: "image/png".into(),
        data: std::sync::Arc::from(""),
    };

    // Completed out of order: the images-bearing one first.
    let none_ready = pending.complete(
        with_images,
        "with images".into(),
        Some(vec![image]),
        ulo_core::extensions::InputVerdict::default(),
    );
    assert!(none_ready.is_empty(), "text_only hasn't completed yet");

    let ordered = pending.complete(
        text_only,
        "text only".into(),
        None,
        ulo_core::extensions::InputVerdict::default(),
    );
    assert_eq!(ordered.len(), 2);
    let (first_text, first_images, _) = &ordered[0];
    assert_eq!(first_text, "text only");
    assert!(first_images.is_none());
    let (second_text, second_images, _) = &ordered[1];
    assert_eq!(second_text, "with images");
    assert_eq!(
        second_images.as_ref().map(Vec::len),
        Some(1),
        "the image must still be attached to its own text, not lost or moved"
    );
}

#[test]
fn a_command_submitted_with_attachments_dispatches_without_them() {
    let mut app = session_app();
    app.attachments.images = vec![ulo_core::providers::ImageInput {
        media_type: "image/png".into(),
        data: std::sync::Arc::from("AA=="),
    }];

    app.submit_composer("/effort high".into());

    assert!(
        app.attachments.images.is_empty(),
        "commands drop attachments"
    );
    // The command dispatched: this model has no effort levels, so the
    // command's own notice replaces a model prompt.
    let notice = app
        .transcript
        .blocks
        .iter()
        .rev()
        .find(|block| block.kind == crate::transcript::Kind::Notice)
        .expect("the command dispatched");
    assert_eq!(notice.text, "this model has no reasoning effort control");
}

#[test]
fn saved_unavailable_models_remain_visible_in_the_scope_picker() {
    let mut app = session_app();
    let missing = "signed-out/model".to_string();
    app.staged_scope = Some(vec![missing.clone()]);
    app.menu = Some(Menu::new(
        MenuKind::Scoped,
        "Scoped models",
        HINT_SCOPED,
        Vec::new(),
    ));

    app.open_scoped_menu();
    let menu = app.menu.as_mut().expect("scope picker remains open");
    menu.select_value(&missing);
    let rendered = menu.render(&app.theme, 80).join("\n");
    assert!(rendered.contains(&missing));
    assert!(rendered.contains("unavailable"));
    assert_eq!(
        app.staged_scope.as_deref(),
        Some(std::slice::from_ref(&missing))
    );

    app.toggle_scoped();
    assert_eq!(app.staged_scope.as_deref(), Some([].as_slice()));
}

#[test]
fn a_paste_over_an_open_surface_stays_text() {
    let mut app = session_app();
    app.viewer = Some(Viewer::new());
    let path = std::env::temp_dir().join("ulo-paste-gate-test.png");
    std::fs::write(&path, b"png").unwrap();

    app.paste(&path.display().to_string());

    assert!(
        app.attachments.images.is_empty(),
        "no attach over a surface"
    );
    assert_eq!(app.editor.text(), path.display().to_string());
}

#[test]
fn a_crlf_paste_keeps_one_newline_per_line() {
    let mut app = session_app();
    app.paste("line1\r\nline2\r\n");
    assert_eq!(app.editor.text(), "line1\nline2\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn enter_waits_for_the_clipboard_result_before_submitting() {
    let mut app = session_app();
    app.attachments.reading = true;

    app.submit_composer("question".into());
    assert_eq!(app.editor.text(), "question");
    assert!(app.attachments.submit_pending);

    app.apply_clipboard_paste(0, Ok(clipboard::Paste::Text(" answer".into())), None);
    assert!(!app.attachments.reading);
    assert!(!app.attachments.submit_pending);
    assert_eq!(app.editor.text(), "");
    app.editor.key(Key::Up);
    assert_eq!(app.editor.text(), "question answer");
}

#[test]
fn deferred_clipboard_submit_expands_long_paste() {
    let mut app = session_app();
    app.reloading = true;
    app.attachments.reading = true;
    let pasted = "x".repeat(1200);

    app.submit_composer("question ".into());
    app.apply_clipboard_paste(0, Ok(clipboard::Paste::Text(pasted.clone())), None);

    assert_eq!(app.held_prompts, vec![format!("question {pasted}")]);
    assert!(app.editor.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_clipboard_result_releases_a_newer_pending_submit() {
    let mut app = session_app();
    app.attachments.reading = true;
    app.discard_composer_images();
    app.submit_composer("new draft".into());

    app.apply_clipboard_paste(0, Ok(clipboard::Paste::Text("stale".into())), None);

    assert_eq!(app.editor.text(), "");
    app.editor.key(Key::Up);
    assert_eq!(app.editor.text(), "new draft");
}

#[test]
fn stale_clipboard_deferred_submit_expands_long_paste() {
    let mut app = session_app();
    app.reloading = true;
    app.attachments.reading = true;
    app.discard_composer_images();
    app.submit_composer("new draft ".into());
    let pasted = "x".repeat(1200);
    app.paste(&pasted);

    app.apply_clipboard_paste(0, Ok(clipboard::Paste::Text("stale".into())), None);

    assert_eq!(app.held_prompts, vec![format!("new draft {pasted}")]);
    assert!(app.editor.is_empty());
    assert!(!app.attachments.reading);
    assert!(!app.attachments.submit_pending);
}

#[tokio::test]
async fn fork_during_a_shell_command_preserves_its_session_and_held_prompts() {
    let home = std::env::temp_dir().join(format!("ulo-fork-shell-{}", uuid::Uuid::new_v4()));
    ulo_core::config::home::with_home(home.clone(), || {
        let mut app = session_app();
        let messages = vec![ulo_core::providers::ChatMessage::user("keep history")];
        let log = ulo_core::session::SessionLog::create_with(
            &app.agent.cwd(),
            &app.agent.model_slug(),
            &messages,
        )
        .unwrap();
        app.agent.set_session(Some(log));
        app.agent.load_history(messages);
        let path = app.agent.session_path();
        let epoch = app.session_epoch;
        app.transcript.push(Block::new(Kind::Shell, "pending"));
        app.shell_block = Some(0);
        app.prompt("after the shell".into());

        app.submit_direct("/fork next".into());

        assert_eq!(app.session_epoch, epoch);
        assert_eq!(app.agent.session_path(), path);
        assert_eq!(app.shell_block, Some(0));
        assert_eq!(app.held_prompts, ["after the shell"]);
        assert!(!app.transcript.blocks[0].done);
        assert!(app
            .transcript
            .blocks
            .last()
            .unwrap()
            .text
            .contains("shell command is running"));
    });
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn clipboard_images_stay_out_of_editable_text_and_get_chat_labels() {
    let mut app = session_app();
    app.agent.model.image_input = true;
    let image = || ulo_core::providers::ImageInput {
        media_type: "image/png".into(),
        data: std::sync::Arc::from("AA=="),
    };

    app.apply_clipboard_paste(
        0,
        Ok(clipboard::Paste::Images(vec![image(), image()])),
        None,
    );

    assert_eq!(app.editor.text(), "");
    assert_eq!(app.attachments.images.len(), 2);
    let attachment_label = app.theme.fg("dim", "[Image 1] [Image 2]");
    assert!(app
        .frame(80, 20)
        .iter()
        .any(|line| line.contains(&attachment_label)));
    assert_eq!(
        display_image_prompt("explain these", 2),
        "[Image 1] [Image 2] explain these"
    );
    assert_eq!(
        display_image_prompt("[Image 1] [Image 2] explain these", 2),
        "[Image 1] [Image 2] explain these"
    );
}

#[test]
fn screenshot_paths_at_the_start_of_a_prompt_are_split_from_the_question() {
    let dir = std::env::temp_dir().join(format!("ulo-shot-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Shotbase Capture.png");
    std::fs::write(&path, b"png").unwrap();
    let input = format!("{} what is wrong here?", path.display());
    let (found, prompt) = leading_image_prompt(&input).expect("image prefix is recognized");
    assert_eq!(std::path::Path::new(found), path);
    assert_eq!(prompt, "what is wrong here?");
    assert!(is_literal_slash_prompt(&input));
    assert!(is_literal_slash_prompt("/ explain this"));
    assert!(!is_literal_slash_prompt("/modles please"));
    let _ = std::fs::remove_dir_all(dir);
}

/// A screenshot paste onto a model that cannot take images must not
/// swallow the user's typed question: the text survives as a normal
/// prompt, with a notice saying the image was dropped.
#[tokio::test(flavor = "multi_thread")]
async fn image_paste_text_survives_a_model_without_image_input() {
    let dir = std::env::temp_dir().join(format!("ulo-shot-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shot.png");
    std::fs::write(&path, b"png").unwrap();

    let mut app = session_app();
    // A dead port: the turn task must never reach a server.
    app.agent.model.base_url = "http://127.0.0.1:1".into();
    assert!(!app.agent.model.image_input);

    app.submit(format!("{} what is wrong here?", path.display()));

    let texts: Vec<&str> = app
        .transcript
        .blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("does not accept image input")),
        "the drop must be announced: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("what is wrong here?")),
        "the question must still reach the model: {texts:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A bare image path with no question has nothing left to submit after
/// the image is dropped, so only the notice appears.
#[tokio::test(flavor = "multi_thread")]
async fn bare_image_path_on_a_text_only_model_only_notices() {
    let dir = std::env::temp_dir().join(format!("ulo-shot-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shot.png");
    std::fs::write(&path, b"png").unwrap();

    let mut app = session_app();
    app.agent.model.base_url = "http://127.0.0.1:1".into();

    app.submit(path.display().to_string());

    let texts: Vec<&str> = app
        .transcript
        .blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect();
    assert_eq!(
        texts.len(),
        1,
        "only the notice, no phantom prompt: {texts:?}"
    );
    assert!(
        texts[0].contains("does not accept image input"),
        "{texts:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The error block supplies its own label, so the event text stays bare.
#[test]
fn failed_turn_does_not_duplicate_the_error_label() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    app.on_session_event(SessionEvent::Error("provider response interrupted".into()));
    app.on_session_event(SessionEvent::TurnEnd { aborted: false });
    let error = app
        .transcript
        .blocks
        .iter()
        .find(|block| block.kind == Kind::Error)
        .unwrap();
    assert_eq!(error.text, "provider response interrupted");
}

/// CRLF is one pasted line break; standalone CR and LF still work.
#[test]
fn paste_normalizes_line_endings_once() {
    let mut app = session_app();
    app.paste("first\r\nsecond\rthird\nfourth");
    assert_eq!(app.editor.text(), "first\nsecond\nthird\nfourth");
}

/// Global cancellation stops an in-flight sign-in and retires secret input.
#[tokio::test]
async fn ctrl_c_cancels_login_before_arming_exit() {
    let mut app = session_app();
    let cancellation = ulo_core::auth::login::LoginCancellation::default();
    let observed = cancellation.clone();
    app.login_task = Some(ActiveLogin {
        flow_id: 1,
        cancellation,
        task: tokio::spawn(std::future::pending()),
        wait_for_callback: false,
    });
    app.auth = Some(AuthStage::Waiting { back: None });
    app.pending_key = Some("mock".into());
    app.editor.mask = true;
    app.editor.set_text("synthetic-secret");
    app.interrupt_or_exit();
    assert!(observed.is_cancelled());
    assert!(app.auth.is_none());
    assert!(app.pending_key.is_none());
    assert!(!app.editor.mask);
    assert!(app.editor.is_empty());
    assert!(!app.should_quit);
    app.interrupt_or_exit();
    assert!(app.should_quit);
}

#[tokio::test]
async fn active_login_guard_cancels_on_drop() {
    let cancellation = ulo_core::auth::login::LoginCancellation::default();
    let observed = cancellation.clone();
    let task = tokio::spawn(std::future::pending());
    let login = ActiveLogin {
        flow_id: 1,
        cancellation,
        task,
        wait_for_callback: false,
    };

    drop(login);
    assert!(observed.is_cancelled());
    tokio::task::yield_now().await;
}

#[test]
fn compaction_uses_the_request_models_pricing_after_a_model_switch() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    app.agent.model.id = "switched".into();
    app.agent.model.pricing = Some(ulo_core::providers::catalog::Pricing {
        input_per_million: 99.0,
        output_per_million: 99.0,
        cache_read_per_million: None,
        cache_write_5m_per_million: None,
        cache_write_1h_per_million: None,
    });
    let request_pricing = ulo_core::providers::catalog::Pricing {
        input_per_million: 2.0,
        output_per_million: 10.0,
        cache_read_per_million: Some(0.2),
        cache_write_5m_per_million: Some(2.5),
        cache_write_1h_per_million: Some(4.0),
    };
    let usage = ulo_core::providers::Usage {
        input: 1_000_000,
        ..Default::default()
    };
    app.on_session_event(SessionEvent::Compacted {
        summary: "summary".into(),
        context_tokens: 10,
        response: ulo_core::providers::ResponseMeta {
            id: "response".into(),
            timestamp: 1,
            provider: "mock".into(),
            model: "m".into(),
            purpose: ulo_core::providers::ResponsePurpose::Compaction,
            usage: Some(usage),
        },
        pricing: Some(request_pricing),
    });

    assert_eq!(app.active.unwrap().cost_usd, Some(2.0));
}

fn session_app() -> App {
    let (agent, _rx) = Agent::new(Model {
        provider: "mock".into(),
        id: "m".into(),
        base_url: "http://localhost".into(),
        api: ulo_core::providers::catalog::Api::Completions,
        catalog: ulo_core::providers::registry::CatalogStrategy::Openai,
        responses_mount: ulo_core::providers::registry::ResponsesMount::Platform,
        provider_supports_tools: true,
        provider_image_input: false,
        effort: Vec::new(),
        thinking: ulo_core::providers::catalog::Thinking::Manual,
        context_window: 200_000,
        max_output: None,
        supports_tools: true,
        image_input: false,
        pricing: None,
    });
    let (jobs, _) = tokio::sync::mpsc::channel(1);
    let (logins, _) = tokio::sync::mpsc::channel(1);
    let (results, _) = tokio::sync::mpsc::channel(1);
    App {
        theme: crate::theme::load_bundled(false).unwrap(),
        keymap: crate::keybindings::Keymap::empty(),
        transcript: Transcript::default(),
        editor: Editor::new(),
        attachments: input::Attachments::default(),
        agent,
        active: None,
        overlay: None,
        armed_at: None,
        should_quit: false,
        context_tokens: 0,
        pending_key: None,
        menu: None,
        staged_scope: None,
        auth: None,
        settings: None,
        show_thinking: true,
        thinking_hint: "Thinking · ctrl o to view".into(),
        jobs,
        logins,
        login_task: None,
        login_sequence: 0,
        host: ulo_core::extensions::ExtensionHost::empty(),
        results,
        input_verdicts: PendingInputVerdicts::default(),
        compacting: false,
        held_prompts: Vec::new(),
        trust: None,
        pending_initial: None,
        pending_initial_images: Vec::new(),
        shell_block: None,
        reloading: false,
        reload_block: None,
        outputs: Vec::new(),
        output_seq: 0,
        viewer: None,
        conversation_scroll: None,
        scroll_lines: 3,
        scroll_hint: "Scrolled · End to follow".into(),
        viewer_cache: None,
        queue_review: None,
        session_epoch: 0,
        update_installed: None,
        relaunch: false,
        rendering_delayed: false,
        last_paint_failure: None,
        light_background: false,
        bottom_pinned: false,
        live_preview_rows: 5,
        tool_label_rows: 2,
        tool_history_limit: 10,
        tool_history_hint: "{count} earlier successful tools · ctrl o to view".into(),
        signed_in: false,
        status_effort: None,
        requests: tokio::sync::mpsc::channel(1).0,
        ui_queue: extui::UiQueue::new(),
        ui_prompt: None,
        ext_status: std::collections::BTreeMap::new(),
        ext_activity: std::collections::BTreeMap::new(),
        ext_panel: None,
        pane: None,
        pane_hidden: false,
        widgets: std::collections::BTreeMap::new(),
        layout: ulo_core::config::layout::Layout::default(),
        external_edit: false,
    }
}

/// A request as the host would deliver it, with a receiver for the
/// reply an extension would read.
fn fake_request(
    extension: &str,
    method: &str,
    params: serde_json::Value,
) -> (
    ulo_core::extensions::HostRequest,
    tokio::sync::oneshot::Receiver<Result<serde_json::Value, String>>,
) {
    ulo_core::extensions::HostRequest::fake(extension, method, params)
}

#[test]
fn show_requests_become_transcript_blocks_and_status_is_bounded() {
    let mut app = session_app();
    let (request, reply) = fake_request(
        "diff",
        "ui.show",
        serde_json::json!({"title": "diff f.txt", "body": "--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n", "format": "diff"}),
    );
    app.on_host_request(request);
    assert_eq!(reply.blocking_recv().unwrap(), Ok(serde_json::json!({})));
    let block = app.transcript.blocks.last().unwrap();
    assert_eq!(block.kind, Kind::Show);
    assert_eq!(block.text, "diff f.txt");
    assert_eq!(
        block.detail.as_deref(),
        Some("f.txt\n    1   a\n    2 - b\n    2 + B")
    );

    let (request, reply) = fake_request(
        "diff",
        "ui.status",
        serde_json::json!({"text": "x".repeat(100) + "\x1b[31m"}),
    );
    app.on_host_request(request);
    assert!(reply.blocking_recv().unwrap().is_ok());
    let status = app.ext_status.get("diff").unwrap();
    assert_eq!(status.chars().count(), 40);
    assert!(status.ends_with('…') && !status.contains('\x1b'));

    let (request, reply) = fake_request("diff", "ui.bogus", serde_json::json!({}));
    app.on_host_request(request);
    assert!(reply
        .blocking_recv()
        .unwrap()
        .unwrap_err()
        .contains("unknown method"));
}

#[test]
fn select_opens_the_picker_and_enter_answers_with_the_value() {
    let mut app = session_app();
    let (request, reply) = fake_request(
        "plan",
        "ui.select",
        serde_json::json!({"title": "Mode", "options": [
            {"label": "Plan", "description": "read only", "value": "plan"},
            "Build"
        ]}),
    );
    app.on_host_request(request);
    let menu = app.menu.as_ref().expect("picker opened");
    assert_eq!(menu.kind, MenuKind::Extension);
    assert_eq!(menu.title, "Mode");
    assert!(app.select_menu());
    assert_eq!(
        reply.blocking_recv().unwrap(),
        Ok(serde_json::json!({"value": "plan", "label": "Plan"}))
    );
    assert!(app.menu.is_none() && app.ui_prompt.is_none());

    // A long plain option answers with the string that was offered,
    // whatever the row showed.
    let long = "/Users/me/projects/very/long/path/to/some/deeply/nested/file_name.rs";
    let (request, reply) = fake_request(
        "plan",
        "ui.select",
        serde_json::json!({"title": "File", "options": [long]}),
    );
    app.on_host_request(request);
    assert!(app.select_menu());
    assert_eq!(reply.blocking_recv().unwrap().unwrap()["value"], long);

    // A second modal while one is open waits its turn; Esc cancels
    // the open one and the next takes the surface on the next frame.
    let (first, first_reply) =
        fake_request("a", "ui.confirm", serde_json::json!({"title": "Sure?"}));
    let (second, second_reply) =
        fake_request("b", "ui.input", serde_json::json!({"title": "Name"}));
    app.on_host_request(first);
    app.on_host_request(second);
    assert_eq!(app.ui_queue.len(), 1);
    app.cancel_ui_prompt();
    app.menu = None;
    assert_eq!(
        first_reply.blocking_recv().unwrap(),
        Ok(serde_json::json!({"confirmed": false}))
    );
    app.pump_ui_queue();
    assert!(app.ui_input_open());
    app.editor.set_text("world");
    app.submit("world".into());
    assert_eq!(
        second_reply.blocking_recv().unwrap(),
        Ok(serde_json::json!({"text": "world"}))
    );
    assert!(!app.ui_input_open() && app.editor.is_empty());
}

#[test]
fn session_tools_narrows_the_agent_and_info_reports_it() {
    let mut app = session_app();
    let (request, reply) = fake_request(
        "plan",
        "session.tools",
        serde_json::json!({"names": ["read", "grep"]}),
    );
    app.on_host_request(request);
    assert!(reply.blocking_recv().unwrap().is_ok());
    assert_eq!(
        app.agent.active_tools(),
        Some(vec!["read".to_string(), "grep".to_string()])
    );
    let (request, reply) = fake_request("plan", "session.info", serde_json::json!({}));
    app.on_host_request(request);
    let info = reply.blocking_recv().unwrap().unwrap();
    assert_eq!(info["tools"], serde_json::json!(["read", "grep"]));
    assert_eq!(info["running"], false);
    let (request, _) = fake_request("plan", "session.tools", serde_json::json!({"names": null}));
    app.on_host_request(request);
    assert_eq!(app.agent.active_tools(), None);
}

#[test]
fn a_command_opened_picker_filters_on_typed_text_and_clears_it_on_close() {
    use crate::menu::{Menu, MenuItem, MenuKind, HINT_USE};
    let mut app = session_app();
    let items = vec![
        MenuItem::new("fix the parser", "", "/a.jsonl"),
        MenuItem::new("write docs", "", "/b.jsonl"),
    ];
    app.menu = Some(Menu::new(MenuKind::Sessions, "Sessions", HINT_USE, items).without_trigger());
    app.editor.set_text("pars");
    app.sync_menu();
    let menu = app.menu.as_ref().unwrap();
    assert_eq!(menu.len(), 1, "typed text is the filter");
    assert_eq!(menu.current().unwrap().value, "/a.jsonl");
    // Esc path: the filter text was never a draft.
    app.menu = None;
    // (the key loop clears the editor for a filtered picker; the
    // selection path does the same through select_menu)
    app.menu = Some(
        Menu::new(
            MenuKind::Tree,
            "Rewind to",
            HINT_USE,
            vec![MenuItem::new("x", "", "n1")],
        )
        .without_trigger(),
    );
    app.editor.set_text("x");
    app.sync_menu();
    assert!(app.select_menu());
    assert!(
        app.editor.is_empty(),
        "the filter does not linger as a draft"
    );
}

#[test]
fn picking_an_argument_completion_replaces_the_typed_prefix() {
    use crate::menu::{Menu, MenuItem, MenuKind, HINT_USE};
    let mut app = session_app();
    app.editor.set_text("/deploy eu st");
    app.menu = Some(Menu::new(
        MenuKind::Arguments,
        "/deploy",
        HINT_USE,
        vec![MenuItem::new("staging", "pre-prod", "staging")],
    ));
    assert!(app.select_menu());
    assert_eq!(app.editor.text(), "/deploy eu staging ");
    assert!(
        app.menu.is_none(),
        "an empty host offers no further completions"
    );
}

#[test]
fn a_pane_splits_the_frame_where_the_layout_says_and_answers_its_owner() {
    let mut app = session_app();
    app.layout = ulo_core::config::layout::parse(
        r#"{"panes":{"diff":{"side":"left","width":40}},"split_min":100}"#,
    )
    .unwrap();
    let (request, reply) = fake_request(
        "diff",
        "ui.pane",
        serde_json::json!({"id": "diff", "title": "Changes", "side": "right", "sections": [
            {"kind": "list", "id": "files", "items": [{"id": "a.rs", "label": "a.rs", "detail": "+1 -0"}]},
            {"kind": "diff", "id": "patch", "body": "@@ -1 +1 @@\n-x\n+y\n"}
        ]}),
    );
    app.on_host_request(request);
    assert!(reply.blocking_recv().unwrap().is_ok());
    let frame = app.frame(120, 20);
    assert_eq!(frame.len(), 20, "a split is a fixed-height frame");
    let plain: Vec<String> = frame
        .iter()
        .map(|r| ulo_core::tools::strip_ansi(r))
        .collect();
    // The layout put the pane on the left at 40%: 48 columns, then the
    // divider, then the conversation.
    assert!(plain[1].starts_with("Changes"), "{:?}", plain[1]);
    assert!(plain[1].contains(" │ "), "{:?}", plain[1]);
    assert_eq!(
        plain[1].split(" │ ").next().unwrap().chars().count(),
        48,
        "{:?}",
        plain[1]
    );
    assert!(plain.iter().any(|r| r.contains("+ y")), "{plain:?}");
    // Too narrow to split: the focused pane fills the frame.
    let narrow = app.frame(80, 20);
    assert!(ulo_core::tools::strip_ansi(&narrow[1]).starts_with("Changes"));
    app.pane.as_mut().unwrap().focused = false;
    let narrow = app.frame(80, 20);
    let plain: Vec<String> = narrow
        .iter()
        .map(|r| ulo_core::tools::strip_ansi(r))
        .collect();
    assert!(
        !plain[1].starts_with("Changes"),
        "unfocused, the conversation shows"
    );
    assert!(
        plain.iter().any(|r| r.contains("Changes pane · ctrl+t")),
        "the status row says how to reach the hidden pane: {plain:?}"
    );
    // A refresh keeps the pane; another extension's pane replaces it;
    // null from the owner closes.
    let (request, _) = fake_request(
        "diff",
        "ui.pane",
        serde_json::json!({"id": "diff", "sections": [{"kind": "text", "body": "clean"}]}),
    );
    app.on_host_request(request);
    assert_eq!(app.pane.as_ref().unwrap().sections.len(), 1);
    let (request, _) = fake_request(
        "plan",
        "ui.pane",
        serde_json::json!({"sections": [{"kind": "text", "body": "steps"}]}),
    );
    app.on_host_request(request);
    assert_eq!(app.pane.as_ref().unwrap().extension, "plan");
    let (request, _) = fake_request("diff", "ui.pane", serde_json::Value::Null);
    app.on_host_request(request);
    assert!(app.pane.is_some(), "only the owner closes a pane");
    let (request, _) = fake_request("plan", "ui.pane", serde_json::Value::Null);
    app.on_host_request(request);
    assert!(app.pane.is_none());
    let (request, reply) = fake_request("plan", "ui.pane", serde_json::json!({"sections": []}));
    app.on_host_request(request);
    assert!(
        reply.blocking_recv().unwrap().is_err(),
        "a pane needs sections"
    );
}

/// The exact request the diff package sends, painted at a real size:
/// every row of the split carries the divider and the pane.
#[test]
fn a_package_shaped_pane_paints_every_row_of_the_split() {
    let mut app = session_app();
    let (request, _) = fake_request(
        "diff",
        "ui.pane",
        serde_json::json!({"id":"diff","title":"3 files changed +6 -4","side":"right","sections":[
            {"kind":"list","id":"files","items":[
                {"id":"list.txt","label":"list.txt","detail":"+1 -1"},
                {"id":"main.rs","label":"main.rs","detail":"+5 -3"},
                {"id":"notes.txt","label":"notes.txt","detail":"new"}],"selected":"list.txt"},
            {"kind":"diff","id":"patch","body":"diff --git a/list.txt b/list.txt\nindex de98044..6372083 100644\n--- a/list.txt\n+++ b/list.txt\n@@ -1,3 +1,3 @@\n a\n-b\n c\n+d"}]}),
    );
    app.on_host_request(request);
    let frame = app.frame(130, 32);
    assert_eq!(frame.len(), 32);
    let plain: Vec<String> = frame
        .iter()
        .map(|r| ulo_core::tools::strip_ansi(r))
        .collect();
    for (i, row) in plain.iter().enumerate() {
        assert!(row.contains(" │ "), "row {i} lost the divider: {row:?}");
        assert!(
            row.chars().count() <= 130,
            "row {i} is wider than the terminal: {row:?}"
        );
        // What the painter does with every row, styled.
        let styled = &frame[i];
        assert!(
            crate::markdown::visible_width(styled) <= 130,
            "row {i} measures wider than the terminal: {styled:?}"
        );
        let _ = crate::markdown::clip_styled(styled, 130);
    }
    assert!(
        plain[3].contains("list.txt") && plain[3].ends_with("+1 -1"),
        "{:?}",
        plain[3]
    );
    assert!(plain.iter().any(|r| r.contains("2 - b")), "{plain:?}");
}

#[test]
fn a_render_answer_rewrites_its_entry_and_a_stale_one_is_dropped() {
    let mut app = session_app();
    let id = app.remember_output("bash".into(), "raw output".into());
    let diff = ulo_core::extensions::Show {
        title: String::new(),
        body: "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n".into(),
        format: ulo_core::extensions::Format::Diff,
    };
    app.apply_render(RenderTarget::Tool(id), diff.clone(), app.session_epoch);
    let body = App::output_body(&app.outputs, id).unwrap();
    assert!(
        body.contains("1 - old") && body.contains("1 + new"),
        "{body:?}"
    );
    // A reply, only when it is still the reply that was asked about.
    app.transcript
        .push(Block::new(Kind::Assistant, "plain reply"));
    let index = app.transcript.blocks.len() - 1;
    let markdown = ulo_core::extensions::Show {
        title: String::new(),
        body: "**bold reply**".into(),
        format: ulo_core::extensions::Format::Markdown,
    };
    app.apply_render(
        RenderTarget::Assistant { index, len: 3 },
        markdown.clone(),
        app.session_epoch,
    );
    assert_eq!(
        app.transcript.blocks[index].text, "plain reply",
        "length mismatch"
    );
    app.apply_render(
        RenderTarget::Assistant {
            index,
            len: "plain reply".len(),
        },
        markdown.clone(),
        app.session_epoch + 1,
    );
    assert_eq!(
        app.transcript.blocks[index].text, "plain reply",
        "epoch mismatch"
    );
    app.apply_render(
        RenderTarget::Assistant {
            index,
            len: "plain reply".len(),
        },
        markdown,
        app.session_epoch,
    );
    assert_eq!(app.transcript.blocks[index].text, "**bold reply**");
}

#[test]
fn the_activity_row_follows_its_template_and_carries_extension_text() {
    let mut app = session_app();
    // Between turns: only the extensions' text, dim, below the transcript.
    let (request, _) = fake_request(
        "tests",
        "ui.activity",
        serde_json::json!({"text": "3 tests running"}),
    );
    app.on_host_request(request);
    let plain: Vec<String> = app
        .transcript_frame(80)
        .iter()
        .map(|r| ulo_core::tools::strip_ansi(r))
        .collect();
    assert_eq!(plain.last().map(String::as_str), Some("3 tests running"));
    // During a turn the template composes the row; the user's template
    // can drop the clock and the tokens.
    app.active = Some(ActiveTurn {
        block: None,
        thinking_block: None,
        turn: Turn::new(),
        started: Instant::now(),
        error: None,
        error_summary: None,
        sleep_stopped: false,
        tool_blocks: std::collections::HashMap::new(),
        tool_names: std::collections::HashMap::new(),
        pending_tools: 0,
        cost_usd: None,
    });
    if let Some(turn) = app.active.as_mut() {
        turn.turn.note_usage(1_000, 20);
    }
    let row = |app: &mut App| -> String {
        let rows = app.transcript_frame(80);
        ulo_core::tools::strip_ansi(rows.last().unwrap())
            .trim()
            .to_string()
    };
    assert_eq!(row(&mut app), "• Thinking (0s) (↑1k ↓20) · 3 tests running");
    app.layout.activity = "{phase} — {activity}".into();
    assert_eq!(row(&mut app), "• Thinking — 3 tests running");
    let (request, _) = fake_request("tests", "ui.activity", serde_json::json!({"text": null}));
    app.on_host_request(request);
    assert_eq!(row(&mut app), "• Thinking —");
}

#[test]
fn an_editor_prompt_takes_a_multi_line_answer() {
    let mut app = session_app();
    let (request, reply) = fake_request(
        "notes",
        "ui.editor",
        serde_json::json!({"title": "Commit message", "text": "first line"}),
    );
    app.on_host_request(request);
    assert!(app.ui_editor_open() && app.ui_input_open());
    assert_eq!(app.editor.text(), "first line");
    assert!(app.answer_ui_input("first line\nsecond line"));
    let answer = reply.blocking_recv().unwrap().unwrap();
    assert_eq!(answer["text"], "first line\nsecond line");
    assert!(!app.ui_input_open());
}

#[test]
fn widgets_sit_above_the_composer_and_keyed_status_fills_the_template() {
    let mut app = session_app();
    let (request, _) = fake_request(
        "plan",
        "ui.widget",
        serde_json::json!({"key": "steps", "lines": [[{"text": "1/3 steps", "token": "accent"}], "next: tests"]}),
    );
    app.on_host_request(request);
    let (request, _) = fake_request(
        "plan",
        "ui.widget",
        serde_json::json!({"key": "clock", "lines": ["12:00"]}),
    );
    app.on_host_request(request);
    let plain: Vec<String> = app
        .frame(80, 20)
        .iter()
        .map(|r| ulo_core::tools::strip_ansi(r))
        .collect();
    // Widgets stack in key order: plan/clock before plan/steps.
    let clock = plain.iter().position(|r| r == "12:00").unwrap();
    assert_eq!(plain[clock + 1], "1/3 steps");
    assert_eq!(plain[clock + 2], "next: tests");
    assert!(
        plain[clock + 3..].iter().any(|r| r.starts_with('┃')),
        "above the composer: {plain:?}"
    );
    let (request, _) = fake_request(
        "plan",
        "ui.widget",
        serde_json::json!({"key": "clock", "lines": null}),
    );
    app.on_host_request(request);
    assert_eq!(app.widgets.len(), 1);

    // Two keyed slots on one extension, joined on the status row; the
    // template can name one extension's alone.
    for (key, text) in [("mode", "plan mode"), ("left", "2 steps left")] {
        let (request, _) = fake_request(
            "plan",
            "ui.status",
            serde_json::json!({"key": key, "text": text}),
        );
        app.on_host_request(request);
    }
    let (request, _) = fake_request("other", "ui.status", serde_json::json!({"text": "busy"}));
    app.on_host_request(request);
    let (_, right) = app.status_segments();
    assert_eq!(right.as_deref(), Some("busy · 2 steps left · plan mode"));
    app.layout.status_right = vec!["{status:plan}".into()];
    let (_, right) = app.status_segments();
    assert_eq!(right.as_deref(), Some("2 steps left · plan mode"));
    app.layout.status_left = vec!["{cwd}".into(), "{model} / {effort}".into()];
    let (left, _) = app.status_segments();
    assert!(!left.is_empty() && !left[0].is_empty(), "{left:?}");
}

#[test]
fn panels_keep_span_spacing_strip_control_bytes_and_paint_tokens() {
    let mut app = session_app();
    let (request, reply) = fake_request(
        "plan",
        "ui.panel",
        serde_json::json!({"title": "Plan", "interactive": true, "lines": [
            [{"text": "› ", "token": "accent"}, {"text": "[ ] ", "token": "dim"}, {"text": "step\x1b[31m one"}],
            "plain\nrow"
        ]}),
    );
    app.on_host_request(request);
    assert!(reply.blocking_recv().unwrap().is_ok());
    let panel = app.ext_panel.as_ref().unwrap();
    assert!(panel.interactive && panel.extension == "plan");
    let rows = panel.render(&app.theme, 80);
    // divider, header, blank, two rows, divider
    assert_eq!(rows.len(), 6);
    assert_eq!(
        rows[3],
        format!(
            "  {}{}step one",
            app.theme.fg("accent", "› "),
            app.theme.fg("dim", "[ ] ")
        )
    );
    assert_eq!(rows[4], "  plain row");
    // Another extension's panel replaces it; a null from the owner closes.
    let (request, _) = fake_request("other", "ui.panel", serde_json::json!({"lines": ["x"]}));
    app.on_host_request(request);
    assert_eq!(app.ext_panel.as_ref().unwrap().title, "other");
    let (request, _) = fake_request("plan", "ui.panel", serde_json::Value::Null);
    app.on_host_request(request);
    assert!(app.ext_panel.is_some(), "only the owner closes a panel");
    let (request, _) = fake_request("other", "ui.panel", serde_json::Value::Null);
    app.on_host_request(request);
    assert!(app.ext_panel.is_none());
}

#[test]
fn queue_review_ignores_an_empty_synchronized_snapshot() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);

    assert!(!app.queue_review_key(KeyCode::Up));
    assert!(app.queue_review.is_none());
    assert!(app.editor.is_empty());
}

#[test]
fn queue_review_commit_leaves_untouched_entries_verbatim() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    // The queue is paused-free: seed it directly, then open the review
    // on the keyed snapshot the commit will edit against.
    app.agent.update_queued(
        vec![(1, "keep  me\n".into()), (2, "old draft".into())],
        vec![],
    );
    let entries = app.agent.queue_snapshot();
    app.queue_review = Some(QueueReview {
        // The untouched entry carries a trailing newline a blanket trim
        // would silently strip; only the edited entry may rewrite.
        entries,
        dirty: vec![false, true],
        selected: 1,
        visible: true,
    });
    app.editor.set_text("  new draft  ");

    assert!(app.queue_review_key(KeyCode::Enter));

    let entries = app.agent.queue_snapshot();
    assert_eq!(
        entries,
        vec![(1, "keep  me\n".to_string()), (2, "new draft".to_string())]
    );
}

#[test]
fn queue_review_commit_drops_an_entry_edited_to_empty() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    app.agent
        .update_queued(vec![(1, "first".into()), (2, "second".into())], vec![]);
    let entries = app.agent.queue_snapshot();
    app.queue_review = Some(QueueReview {
        entries,
        dirty: vec![false, true],
        selected: 1,
        visible: true,
    });
    app.editor.set_text("   ");

    assert!(app.queue_review_key(KeyCode::Enter));

    let entries = app.agent.queue_snapshot();
    assert_eq!(entries, vec![(1, "first".to_string())]);
}

#[test]
fn full_reader_opens_at_tail_and_keeps_a_paused_reading_position() {
    let mut app = session_app();
    app.editor.set_text("unsent draft");
    for i in 0..30 {
        app.transcript
            .push(Block::new(Kind::User, format!("message {i}")));
    }
    app.viewer = Some(Viewer::new());
    let frame = app.viewer_frame(80, 24);
    assert_eq!(frame.len(), 24);
    assert!(frame[..21].iter().any(|row| row.contains("message 29")));
    assert!(frame[21].contains("Review"));
    assert_eq!(frame[22], "");
    app.scroll_viewer(false, 5, 80, 24);
    let paused = app.viewer_frame(80, 24);
    app.transcript.push(Block::new(Kind::User, "new arrival"));
    assert_eq!(app.viewer_frame(80, 24), paused);
    app.scroll_viewer(true, usize::MAX, 80, 24);
    assert!(app
        .viewer_frame(80, 24)
        .iter()
        .any(|row| row.contains("new arrival")));
    assert!(app.viewer_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 80, 24));
    assert!(app.viewer.is_none());
    assert_eq!(app.editor.text(), "unsent draft");
}

#[test]
fn review_screen_rebuilds_after_transcript_changes() {
    let mut app = session_app();
    app.viewer = Some(Viewer {
        follow_tail: false,
        scroll: 0,
        ..Viewer::new()
    });
    app.transcript
        .push(Block::new(Kind::User, "before the change"));
    let before = app.viewer_rows(80, false).to_vec();

    // Same width and depth, new block: the cache must notice.
    app.transcript
        .push(Block::new(Kind::User, "after the change"));
    let after = app.viewer_rows(80, false).to_vec();

    assert_eq!(after.len(), before.len() + 2, "block plus its gap row");
    assert!(after.last().unwrap().contains("after the change"));
}

#[test]
fn review_screen_rebuilds_when_a_tool_reports() {
    let mut app = session_app();
    app.viewer = Some(Viewer {
        follow_tail: false,
        scroll: 0,
        ..Viewer::new()
    });
    let child = crate::transcript::ToolChild::pending(
        7,
        "command".into(),
        "Running true".into(),
        "Ran true".into(),
        "true".into(),
    );
    let block = app.transcript.push(Block::tool_group(vec![child]));
    app.on_session_event(SessionEvent::TurnStart);
    app.on_session_event(SessionEvent::ToolStart { id: 7 });
    let running = app.viewer_rows(80, false).to_vec();

    app.active.as_mut().unwrap().tool_blocks.insert(7, block);
    app.on_session_event(SessionEvent::ToolEnd {
        id: 7,
        outcome: ulo_core::tools::ToolOutcome::Completed,
        summary: "done".into(),
        content: "full saved output".into(),
    });
    let reported = app.viewer_rows(80, false).to_vec();

    assert!(
        reported.iter().any(|row| row.contains("full saved output")),
        "the new detail must appear without a width or depth change"
    );
    assert_ne!(running, reported);
}

#[test]
fn review_screen_clamps_scroll_when_the_body_shrinks() {
    let mut app = session_app();
    app.transcript.push(Block::new(Kind::User, "some content"));
    app.viewer = Some(Viewer {
        follow_tail: false,
        scroll: 500,
        ..Viewer::new()
    });

    let frame = app.viewer_frame(80, 24);

    let body = &frame[..frame.len() - 1];
    assert!(
        body.iter().any(|r| r.contains("some content")),
        "a deep scroll must clamp to the shrunken body: {frame:?}"
    );
    assert_eq!(
        app.viewer.as_ref().unwrap().scroll,
        0,
        "the clamp persists so ↑/↓ arithmetic starts in range"
    );
}

/// Review closes each group after its inserted output, at either depth.
#[test]
fn review_branches_connect_through_output_and_omission_rows() {
    let mut app = session_app();
    let detail = app.remember_output("output".into(), "one\ntwo\nthree\nfour".into());
    let mut group = Block::tool_group(
        (1..=2)
            .map(|id| {
                crate::transcript::ToolChild::pending(
                    id,
                    "command".into(),
                    "Running".into(),
                    "Ran".into(),
                    format!("command {id}\nwrapped argument"),
                )
            })
            .collect(),
    );
    for id in 1..=2 {
        group.start_tool(id);
        group.finish_tool(
            id,
            ulo_core::tools::ToolOutcome::Completed,
            "done".into(),
            "",
        );
    }
    for child in &mut group.tool_children {
        child.detail = Some(detail);
    }
    app.transcript.push(group);
    for full in [false, true] {
        let rows: Vec<_> = app
            .viewer_rows(80, full)
            .iter()
            .map(|row| ulo_core::tools::strip_ansi(row))
            .collect();
        assert_eq!(rows.iter().filter(|row| row.starts_with('└')).count(), 1);
        assert_eq!(rows.iter().filter(|row| row.starts_with('├')).count(), 2);
        assert_eq!(rows[2], "│ wrapped argument");
        assert_eq!(
            rows.last().unwrap(),
            if full {
                "└ four"
            } else {
                "└ 1 more rows · → to expand"
            }
        );
        assert!(rows[1..rows.len() - 1]
            .iter()
            .all(|row| row.starts_with('├') || row.starts_with('│')));
    }
}

#[test]
fn tui_mode_defaults_inline_and_settings_cycle_the_layout() {
    let home = std::env::temp_dir().join(format!("ulo-composer-{}", uuid::Uuid::new_v4()));
    ulo_core::config::home::with_home(home.clone(), || {
        let mut app = session_app();
        app.refresh_status_cache();
        assert!(!app.bottom_pinned);
        assert!(app.frame(80, 30).len() < 30);

        let setting = ulo_core::config::settings::all(Vec::new())
            .into_iter()
            .find(|setting| setting.key == "tui_mode")
            .unwrap();
        assert_eq!(setting.current(), "inline");
        ulo_core::config::settings::set_string("composer_position", "bottom").unwrap();
        app.refresh_status_cache();
        assert!(app.bottom_pinned);
        assert_eq!(setting.current(), "fullscreen");
        setting.cycle(-1).unwrap();
        assert_eq!(setting.current(), "inline");
        setting.cycle(1).unwrap();
        app.refresh_status_cache();
        assert!(app.bottom_pinned);
        assert_eq!(app.frame(80, 30).len(), 30);

        setting.cycle(-1).unwrap();
        app.refresh_status_cache();
        assert!(!app.bottom_pinned);
        assert!(app.frame(80, 30).len() < 30);

        ulo_core::config::settings::set_string("tui_mode", "invalid").unwrap();
        app.refresh_status_cache();
        assert_eq!(setting.current(), "inline");
        assert!(!app.bottom_pinned);
    });
    std::fs::remove_dir_all(home).unwrap();
}

/// Reloaded label budgets invalidate existing frames and apply to new groups.
#[test]
fn tool_label_preference_updates_existing_and_new_groups() {
    let home = std::env::temp_dir().join(format!("ulo-tool-labels-{}", uuid::Uuid::new_v4()));
    ulo_core::config::home::with_home(home.clone(), || {
        let mut app = session_app();
        app.refresh_status_cache();
        app.on_session_event(SessionEvent::TurnStart);
        let batch = || SessionEvent::ToolBatchStart {
            calls: vec![ulo_core::agent::ToolCallPresentation {
                id: 1,
                name: "bash".into(),
                arguments: "{}".into(),
                category: "command".into(),
                running: "Running".into(),
                completed: "Ran".into(),
                target: "long-command".repeat(20),
            }],
        };
        app.on_session_event(batch());
        app.on_session_event(SessionEvent::ToolStart { id: 1 });
        let original = app.transcript.blocks[0].lines_for_test(&app.theme, 40);
        ulo_core::config::store::update_versioned(
            &home.join("settings.json"),
            0o644,
            1,
            |settings| {
                settings.insert("tool_label_rows".into(), serde_json::json!(1));
            },
        )
        .unwrap();
        app.refresh_status_cache();
        let shorter = app.transcript.blocks[0].lines_for_test(&app.theme, 40);
        assert_eq!(shorter.len() + 1, original.len());
        app.notice("separate group".into());
        app.on_session_event(batch());
        assert_eq!(app.transcript.blocks.last().unwrap().tool_label_rows, 1);
    });
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn command_output_and_completion_do_not_move_the_composer_dock() {
    let mut app = session_app();
    app.bottom_pinned = true;
    app.on_session_event(SessionEvent::TurnStart);
    app.on_session_event(SessionEvent::ToolBatchStart {
        calls: vec![ulo_core::agent::ToolCallPresentation {
            id: 1,
            name: "bash".into(),
            arguments: "{}".into(),
            category: "command".into(),
            running: "Running".into(),
            completed: "Ran".into(),
            target: "test command with a long argument".into(),
        }],
    });
    app.on_session_event(SessionEvent::ToolStart { id: 1 });
    for (width, height, count) in [(80, 24, 1), (24, 12, 30), (80, 24, 2)] {
        app.on_session_event(SessionEvent::ToolOutput {
            id: 1,
            stream: ulo_core::tools::OutputStream::Stdout,
            chunk: "output line with a long argument\n".repeat(count),
        });
        let frame = app.frame(width, height);
        assert!(frame.len() >= height);
        assert!(ulo_core::tools::strip_ansi(&frame[frame.len() - 3]).starts_with("┃ "));
        let review = app.viewer_rows(width, true).join("\n");
        assert!(
            review.contains("output line"),
            "running output must be reviewable"
        );
    }
    app.on_session_event(SessionEvent::ToolEnd {
        id: 1,
        outcome: ulo_core::tools::ToolOutcome::Completed,
        summary: "done".into(),
        content: "authoritative final output".into(),
    });
    app.on_session_event(SessionEvent::TurnEnd { aborted: false });
    let frame = app.frame(80, 24);
    assert_eq!(frame.len(), 24);
    assert!(ulo_core::tools::strip_ansi(&frame[21]).starts_with("┃ "));
    assert!(app
        .viewer_rows(80, true)
        .join("\n")
        .contains("authoritative final output"));
}

#[test]
fn queued_banner_offers_edit_only_for_editable_prompts() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    // A held prompt (compaction, a `!` command) cannot be pulled into
    // the composer — only queued steering prompts can.
    app.held_prompts = vec!["held".into()];

    let lines = app.frame(80, 24);

    let banner = lines
        .iter()
        .find(|l| l.contains("queued message"))
        .expect("the banner names the held prompt");
    assert!(!banner.contains("↑ to edit"), "{banner:?}");
}

#[test]
fn cancelled_turn_discards_the_reviewed_prompt_from_composer() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    app.queue_review = Some(QueueReview {
        entries: vec![(9, "original".into())],
        dirty: vec![false],
        selected: 0,
        visible: true,
    });
    app.editor.set_text("edited draft");

    app.on_session_event(SessionEvent::TurnEnd { aborted: true });

    assert!(app.queue_review.is_none());
    assert!(app.editor.is_empty());
}

#[test]
fn rejected_image_suffixes_remain_literal_prompts() {
    let image =
        std::env::temp_dir().join(format!("ulo-image-command-{}.png", uuid::Uuid::new_v4()));
    std::fs::write(&image, b"image placeholder").unwrap();
    for suffix in ["/new", "/quit", "!touch should-not-run"] {
        let mut app = session_app();
        app.agent
            .load_history(vec![ulo_core::providers::ChatMessage::user("keep history")]);
        // Hold literal prompts locally without starting a provider request.
        app.reloading = true;
        app.submit_direct(format!("{} {suffix}", image.display()));
        assert_eq!(app.held_prompts, [suffix]);
        assert!(!app.should_quit);
        assert!(app.shell_block.is_none());
        assert_eq!(app.agent.history_snapshot()[0].content, "keep history");
    }
    std::fs::remove_file(image).unwrap();
}

#[test]
fn stale_tool_lifecycle_does_not_change_the_current_turn() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    app.on_session_event(SessionEvent::ToolStart { id: 99 });
    assert!(matches!(
        app.active.as_ref().unwrap().turn.phase,
        TurnPhase::Waiting
    ));
    app.on_session_event(SessionEvent::ToolBatchStart {
        calls: vec![ulo_core::agent::ToolCallPresentation {
            id: 2,
            name: "bash".into(),
            arguments: "{}".into(),
            category: "command".into(),
            running: "Running".into(),
            completed: "Ran".into(),
            target: "current".into(),
        }],
    });
    app.on_session_event(SessionEvent::ToolEnd {
        id: 99,
        outcome: ulo_core::tools::ToolOutcome::Completed,
        summary: "late".into(),
        content: "old output".into(),
    });
    let active = app.active.as_ref().unwrap();
    assert_eq!(active.pending_tools, 1);
    assert!(matches!(active.turn.phase, TurnPhase::Tool));
    assert!(app.outputs.is_empty());
}

#[test]
fn streamed_deltas_append_to_the_active_transcript_block() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    app.on_session_event(SessionEvent::TextDelta("first ".into()));
    app.on_session_event(SessionEvent::TextDelta("second".into()));

    let replies: Vec<_> = app
        .transcript
        .blocks
        .iter()
        .filter(|block| block.kind == Kind::Assistant)
        .collect();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].text, "first second");
}

fn thinking_flags(app: &App) -> Vec<(String, bool)> {
    app.transcript
        .blocks
        .iter()
        .filter(|block| block.kind == Kind::Thinking)
        .map(|block| (block.text.clone(), block.done))
        .collect()
}

#[test]
fn help_picker_filters_without_a_slash_trigger() {
    let mut app = session_app();
    app.submit_direct("/help".into());

    app.editor.set_text("res");
    app.sync_menu();
    let menu = app
        .menu
        .as_ref()
        .expect("help picker stays open while typing");
    assert_eq!(
        menu.current().map(|item| item.value.as_str()),
        Some("/resume")
    );

    app.editor.set_text("vers");
    app.sync_menu();
    let menu = app
        .menu
        .as_ref()
        .expect("help picker stays open after paste");
    assert_eq!(
        menu.current().map(|item| item.value.as_str()),
        Some("/version")
    );
}

#[test]
fn transcript_rebuild_discards_previous_output_details() {
    let mut app = session_app();
    app.remember_output("old session".into(), "old detail".into());
    app.rebuild_transcript(&[]);
    assert!(app.outputs.is_empty());
}

fn tool_batch() -> SessionEvent {
    SessionEvent::ToolBatchStart {
        calls: vec![ulo_core::agent::ToolCallPresentation {
            id: 1,
            name: "read".into(),
            arguments: "{\"path\":\"f.rs\"}".into(),
            category: "read".into(),
            running: "reading".into(),
            completed: "read".into(),
            target: "f.rs".into(),
        }],
    }
}

/// A typical think-then-tools turn opens a second thinking burst below
/// the tools. Ending a burst — tools taking over, or TurnEnd — detaches it
/// but leaves the thought expanded where it sits: no collapse to a
/// `Thought for Ns` row, and never marked done (which would shrink the
/// frame and jump the screen).
#[test]
fn thinking_bursts_stay_expanded_at_each_handoff() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    app.on_session_event(SessionEvent::ReasoningDelta("before tools".into()));
    assert_eq!(thinking_flags(&app), vec![("before tools".into(), false)]);

    app.on_session_event(tool_batch());
    assert_eq!(
        thinking_flags(&app),
        vec![("before tools".into(), false)],
        "the pre-tool burst stays expanded when tools take over"
    );

    app.on_session_event(SessionEvent::ReasoningDelta("after tools".into()));
    assert_eq!(
        thinking_flags(&app),
        vec![
            ("before tools".into(), false),
            ("after tools".into(), false)
        ],
        "a fresh burst opens its own block below the tools"
    );

    // Continuing tools must not absorb expanded reasoning as though it
    // were an old collapsed summary, shrinking the transcript mid-turn.
    app.on_session_event(tool_batch());
    assert_eq!(thinking_flags(&app).len(), 2);

    app.on_session_event(SessionEvent::TurnEnd { aborted: false });
    assert_eq!(
        thinking_flags(&app),
        vec![
            ("before tools".into(), false),
            ("after tools".into(), false)
        ],
        "TurnEnd leaves both thoughts expanded, unchanged"
    );
}

/// Retries and steered messages also end the live burst; each thought
/// keeps its expanded place, and the next burst opens a new block.
#[test]
fn thinking_stays_expanded_at_retry_and_steer() {
    let mut app = session_app();
    app.on_session_event(SessionEvent::TurnStart);
    app.on_session_event(SessionEvent::ReasoningDelta("attempt one".into()));
    app.on_session_event(SessionEvent::Retry {
        attempt: 1,
        limit: 3,
        delay_secs: 1,
        cause: ulo_core::providers::FailureCause::Network,
        reason: "timeout".into(),
    });
    assert_eq!(
        thinking_flags(&app),
        vec![("attempt one".into(), false)],
        "the abandoned attempt's thought stays put at the retry"
    );

    app.on_session_event(SessionEvent::ReasoningDelta("attempt two".into()));
    app.on_session_event(SessionEvent::Steered("also check this".into()));
    app.on_session_event(SessionEvent::ReasoningDelta("after steer".into()));
    assert_eq!(
        thinking_flags(&app),
        vec![
            ("attempt one".into(), false),
            ("attempt two".into(), false),
            ("after steer".into(), false)
        ]
    );

    app.on_session_event(SessionEvent::TurnEnd { aborted: false });
    assert_eq!(
        thinking_flags(&app),
        vec![
            ("attempt one".into(), false),
            ("attempt two".into(), false),
            ("after steer".into(), false)
        ]
    );
}
