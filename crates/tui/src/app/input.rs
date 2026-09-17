//! Handle composer input, attachments, and prompt submission.

use super::runtime::arm;
use super::*;

/// Attachments belong to one draft; generation rejects late clipboard reads.
#[derive(Default)]
pub(super) struct Attachments {
    pub(super) images: Vec<e_core::providers::ImageInput>,
    pub(super) generation: u64,
    pub(super) reading: bool,
    pub(super) submit_pending: bool,
}

impl App {
    pub(super) fn interrupt_or_exit(&mut self) {
        if self
            .armed_at
            .is_some_and(|at| at.elapsed() < Duration::from_millis(1500))
        {
            self.should_quit = true;
            return;
        }
        if self.agent.is_streaming() {
            self.agent.interrupt();
        }
        self.cancel_login();
        // An extension's modal is answered "cancelled", never left waiting
        // behind a surface that is gone.
        self.cancel_ui_prompt();
        self.close_ext_panel(true);
        self.close_pane(true);
        self.auth = None;
        self.trust = None;
        self.queue_review = None;
        self.pending_initial = None;
        self.pending_initial_images.clear();
        self.pending_key = None;
        self.editor.mask = false;
        self.editor.set_text("");
        self.discard_composer_images();
        self.viewer = None;
        self.settings = None;
        self.menu = None;
        self.staged_scope = None;
        arm(self);
    }

    /// Insert text normally, or turn a pasted list of image paths into
    /// attachments — but only into a free composer: over an open surface a
    /// paste is plain text, so it cannot silently stack onto a draft the
    /// user is not looking at. Line endings normalise to `\n`: CRLF first,
    /// so a Windows clipboard does not double every line, then the bare CR
    /// some terminals send for a pasted newline.
    pub(super) fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if self.composer_free() {
            let paths: Vec<String> = text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(String::from)
                .collect();
            let all_files = !paths.is_empty()
                && paths
                    .iter()
                    .all(|path| std::path::Path::new(path).is_file());
            if all_files && self.agent.model.image_input {
                // The reads run off the event loop — a slow or networked
                // file must not stall input and repaint. Stale results are
                // dropped by the draft generation, like a clipboard read;
                // a read that cannot attach restores the pasted text.
                let generation = self.attachments.generation;
                let results = self.results.clone();
                let fallback = Some(text.clone());
                e_core::config::home::spawn(async move {
                    let images = tokio::task::spawn_blocking(move || {
                        e_core::providers::ImageInput::from_paths(&paths)
                    })
                    .await
                    .unwrap_or_else(|_| Err("image attachment reader panicked".into()));
                    let _ = results
                        .send(AppJob::ClipboardPaste {
                            generation,
                            paste: images.map(clipboard::Paste::Images),
                            fallback,
                        })
                        .await;
                });
                return;
            }
        }
        self.editor.insert_paste(&text);
        self.sync_menu();
    }

    /// True when nothing overlays the composer and a paste may attach to it.
    pub(super) fn composer_free(&self) -> bool {
        self.viewer.is_none()
            && self.menu.is_none()
            && self.settings.is_none()
            && self.auth.is_none()
            && self.trust.is_none()
            && self.queue_review.is_none()
    }

    /// Forget attachments with a discarded or replaced composer draft.
    pub(super) fn discard_composer_images(&mut self) {
        self.attachments.images.clear();
        self.attachments.submit_pending = false;
        self.attachments.generation = self.attachments.generation.wrapping_add(1);
    }

    /// Put a paste back into the composer when its images could not load —
    /// the text is the user's, whether or not it turned into attachments.
    pub(super) fn restore_fallback(&mut self, fallback: Option<String>) {
        if let Some(text) = fallback {
            self.editor.insert_paste(&text);
            self.sync_menu();
        }
    }

    /// Start a bounded clipboard read without blocking terminal input. One
    /// read at a time — a second paste while one is in flight is declined
    /// rather than stacked, so a slow helper cannot accumulate waiters.
    pub(super) fn paste_clipboard(&mut self) {
        if self.attachments.reading {
            return;
        }
        self.attachments.reading = true;
        let generation = self.attachments.generation;
        let results = self.results.clone();
        e_core::config::home::spawn(async move {
            let paste = tokio::task::spawn_blocking(clipboard::read)
                .await
                .unwrap_or_else(|_| Err("clipboard reader panicked".into()));
            let _ = results
                .send(AppJob::ClipboardPaste {
                    generation,
                    paste,
                    fallback: None,
                })
                .await;
        });
    }

    /// Apply a clipboard or pasted-path result to the current draft. Attachment
    /// state renders outside the editor so history can never contain fake labels.
    pub(super) fn apply_clipboard_paste(
        &mut self,
        generation: u64,
        paste: Result<clipboard::Paste, String>,
        fallback: Option<String>,
    ) {
        // Only a direct clipboard job owns these flags; a path-paste read
        // never set them.
        let submit_after = if fallback.is_none() {
            self.attachments.reading = false;
            std::mem::take(&mut self.attachments.submit_pending)
        } else {
            false
        };
        if generation != self.attachments.generation {
            // The old payload cannot enter a newer draft, but Enter may have
            // been pressed on that draft while this stale read still owned the
            // in-flight flag. Release the requested submission now.
            if submit_after {
                let text = self.editor.expanded_text();
                self.editor.set_text("");
                self.submit_composer(text);
            }
            return;
        }
        match paste {
            Ok(clipboard::Paste::Images(images)) if !images.is_empty() => {
                let mut batch = self.attachments.images.clone();
                batch.extend(images.iter().cloned());
                if let Err(error) = e_core::providers::ImageInput::validate_batch(&batch) {
                    self.notice(error);
                    self.restore_fallback(fallback);
                } else if self.agent.model.image_input {
                    self.attachments.images.extend(images);
                    self.sync_menu();
                } else {
                    self.notice(format!(
                        "{} does not accept image input",
                        model::slug(&self.agent.model)
                    ));
                }
            }
            Ok(clipboard::Paste::Images(_)) => {}
            Ok(clipboard::Paste::Text(text)) => {
                self.editor.insert_paste(&text);
                self.sync_menu();
            }
            Err(error) => {
                self.notice(error);
                self.restore_fallback(fallback);
            }
        }
        if submit_after {
            let text = self.editor.expanded_text();
            self.editor.set_text("");
            self.submit_composer(text);
        }
    }

    /// Submit the visible draft with any clipboard images attached to it.
    pub(super) fn submit_composer(&mut self, text: String) {
        if self.attachments.reading {
            self.editor.set_text(&text);
            self.attachments.submit_pending = true;
            return;
        }
        if self.attachments.images.is_empty() {
            if !text.trim().is_empty() {
                self.attachments.generation = self.attachments.generation.wrapping_add(1);
            }
            self.submit(text);
            return;
        }
        // A command or shell line never carries images: route the text
        // through the normal dispatch and drop the attachments — they were
        // attached to a draft, and the dispatch owns what happens to it.
        let trimmed = text.trim();
        if trimmed.starts_with('/') || trimmed.starts_with('!') {
            self.discard_composer_images();
            self.notice("commands do not carry image attachments".into());
            self.submit(text);
            return;
        }
        if !self.agent.model.image_input {
            self.editor.set_text(&text);
            self.notice(format!(
                "{} does not accept image input",
                model::slug(&self.agent.model)
            ));
            return;
        }
        if self.agent.is_streaming()
            || self.compacting
            || self.reloading
            || self.shell_block.is_some()
        {
            self.editor.set_text(&text);
            self.notice("send image prompts between turns".into());
            return;
        }
        self.attachments.generation = self.attachments.generation.wrapping_add(1);
        let images = std::mem::take(&mut self.attachments.images);
        self.submit_images(text, images);
    }

    /// Submit attached images through the same ordered input-hook path as text.
    pub(super) fn submit_images(
        &mut self,
        text: String,
        images: Vec<e_core::providers::ImageInput>,
    ) {
        let trimmed = text.trim();
        let prompt = if trimmed.is_empty() {
            "Describe this image.".to_string()
        } else {
            trimmed.to_string()
        };
        // History is recorded where the prompt is actually accepted — the
        // hook may consume or replace this text.
        if self.host.has_input_hook() {
            let host = self.host.clone();
            let results = self.results.clone();
            let sequence = self.input_verdicts.reserve();
            e_core::config::home::spawn(async move {
                let verdict = host.hook_input(&prompt).await;
                let _ = results
                    .send(AppJob::InputVerdict {
                        sequence,
                        text: prompt,
                        images: Some(images),
                        verdict,
                    })
                    .await;
            });
        } else {
            self.submit_with_images(prompt, images);
        }
    }

    pub(super) fn submit(&mut self, text: String) {
        let trimmed = text.trim().to_string();
        // An extension's question takes the line whole — even an empty one
        // is an answer — and it never reaches input hooks or the model.
        if self.ui_input_open() {
            self.answer_ui_input(&text);
            return;
        }
        if trimmed.is_empty() {
            return;
        }

        let route = input_route(self.pending_key.is_some(), self.host.has_input_hook());
        // API keys are consumed before extension dispatch, matching the
        // documented boundary that secrets never reach input hooks.
        if route == InputRoute::ApiKey {
            self.submit_api_key(&trimmed);
            return;
        }

        // An input hook can consume or rewrite the line before anything else
        // sees it. Completed calls are applied in submission order below.
        if route == InputRoute::Hook {
            let host = self.host.clone();
            let results = self.results.clone();
            let sequence = self.input_verdicts.reserve();
            e_core::config::home::spawn(async move {
                let verdict = host.hook_input(&trimmed).await;
                let _ = results
                    .send(AppJob::InputVerdict {
                        sequence,
                        text,
                        images: None,
                        verdict,
                    })
                    .await;
            });
            return;
        }
        self.submit_direct(trimmed);
    }

    pub(super) fn apply_input_verdict(
        &mut self,
        text: String,
        images: Option<Vec<e_core::providers::ImageInput>>,
        verdict: e_core::extensions::InputVerdict,
    ) {
        if let Some(notice) = verdict.notice.filter(|n| !n.trim().is_empty()) {
            self.notice(notice);
        }
        if verdict.consume {
            // Swallowed entirely. Any attached images are dropped with the
            // text because there is no accepted prompt left to carry them.
        } else if let Some(replace) = verdict.replace {
            // The extension rewrote the line; it already saw the original, so
            // no second hook pass.
            match images {
                Some(images) if !images.is_empty() => self.submit_with_images(replace, images),
                _ => self.submit_direct(replace),
            }
        } else {
            // Allowed through — the hook already saw the text, so submit
            // directly. Re-running submit() here would loop through the hook.
            match images {
                Some(images) if !images.is_empty() => self.submit_with_images(text, images),
                _ => self.submit_direct(text),
            }
        }
    }

    /// The real submit flow, after the input hook (if any) has had its say.
    pub(super) fn submit_direct(&mut self, text: String) {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return;
        }

        if let Some((path, prompt)) = leading_image_prompt(&trimmed) {
            // The successful branch records history in submit_with_images,
            // with the text that actually went to the model; these falls
            // record the original line.
            if !self.agent.model.image_input {
                // The image cannot ride along, but the question after the
                // path is still the user's prompt — discarding it and
                // stopping the turn would swallow the typed message along
                // with the attachment.
                if prompt.is_empty() {
                    self.remember_prompt(text);
                    self.notice(format!(
                        "{} does not accept image input",
                        model::slug(&self.agent.model)
                    ));
                } else {
                    self.remember_prompt(text);
                    self.notice(format!(
                        "{} does not accept image input — sending the text without the screenshot",
                        model::slug(&self.agent.model)
                    ));
                    self.prompt(prompt.to_string());
                }
                return;
            }
            match e_core::providers::ImageInput::from_path(std::path::Path::new(path)) {
                Ok(image) => {
                    let prompt = if prompt.is_empty() {
                        "Describe this image.".to_string()
                    } else {
                        prompt.to_string()
                    };
                    self.submit_with_images(prompt, vec![image]);
                }
                Err(error) => {
                    self.remember_prompt(text);
                    self.notice(format!("could not attach image: {error}"));
                }
            }
            return;
        }

        self.remember_prompt(text);

        // `!cmd` runs in the shell directly; the output lands in the
        // transcript and in history, so the model sees what the user did.
        if let Some(cmd) = trimmed.strip_prefix('!').map(str::trim) {
            if !cmd.is_empty() {
                self.run_shell(cmd.to_string());
                return;
            }
        }

        if let Some(rest) = command_arg(&trimmed, "/login") {
            let provider = rest.trim().to_string();
            if provider.is_empty() {
                self.open_login_menu();
            } else {
                self.login(provider);
            }
            return;
        }

        if trimmed == "/scoped-models" {
            self.open_scoped_menu();
            return;
        }
        if let Some(rest) = command_arg(&trimmed, "/effort") {
            let requested = rest.trim();
            let levels = self.agent.effort_levels();
            if levels.is_empty() {
                self.notice("this model has no reasoning effort control".into());
            } else if requested.is_empty() {
                let current = self.agent.effort().unwrap_or_else(|| levels[0].clone());
                self.notice(format!(
                    "reasoning effort is {current} · available: {} · use /effort <level>",
                    levels.join(", ")
                ));
            } else if !levels.iter().any(|level| level == requested) {
                self.notice(format!(
                    "unsupported reasoning effort {requested:?} · available: {}",
                    levels.join(", ")
                ));
            } else {
                match self.agent.set_effort(requested) {
                    Ok(true) => {
                        self.refresh_status_cache();
                        self.notice(format!("reasoning effort set to {requested}"));
                    }
                    Ok(false) => self.notice("this model has no reasoning effort control".into()),
                    Err(error) => self.notice(format!("could not save reasoning effort: {error}")),
                }
            }
            return;
        }
        let model_rest =
            command_arg(&trimmed, "/models").or_else(|| command_arg(&trimmed, "/model"));
        if let Some(rest) = model_rest {
            let query = rest.trim();
            if query.is_empty() {
                self.open_model_menu();
            } else if let Some(found) = model::resolve(query) {
                if let Err(error) = persist_model(&found) {
                    self.notice(format!("could not save model choice: {error}"));
                    return;
                }
                self.notice(format!("model set to {}", model::slug(&found)));
                self.agent.model = found;
                self.refresh_status_cache();
            } else {
                self.notice(format!(
                    "no available model matches {query:?} — sign in to its provider with /login"
                ));
            }
            return;
        }
        match trimmed.as_str() {
            "/quit" | "/exit" => self.should_quit = true,
            "/version" => self.notice(format!("e {}", e_core::VERSION)),
            "/help" => {
                // The reference help surface is the commands picker itself —
                // browse, filter, Enter to use — not a wall of text. The
                // non-command shortcuts ride the transcript as one notice so
                // they stay discoverable.
                self.notice(
                    "! <cmd> runs a shell command (the model sees the output) · \
                     shift+tab cycles reasoning effort · ctrl+v attaches clipboard images · \
                     ctrl+o opens full tool detail"
                        .into(),
                );
                self.menu = Some(
                    crate::menu::Menu::new(
                        crate::menu::MenuKind::Commands,
                        "Commands",
                        crate::menu::HINT_USE,
                        self.command_items(),
                    )
                    .without_trigger(),
                );
            }
            "/new" | "/clear" => {
                // A running turn owns the history and session log; replacing
                // them mid-turn would commit its reply into the wrong
                // session. `is_streaming` also covers the gap between a
                // submit and its TurnStart event.
                if self.active.is_some() || self.agent.is_streaming() {
                    self.notice("a turn is running — press Esc to stop it, then /new".into());
                    return;
                }
                self.compacting = false;
                self.held_prompts.clear();
                self.shell_block = None;
                self.reload_block = None;
                self.context_tokens = 0;
                self.discard_composer_images();
                self.agent.clear();
                self.agent.clear_session_name();
                self.agent.set_session(None);
                // The name is part of session identity: a fresh session must
                // not inherit the old one's.
                self.agent.adopt_session_name(None);
                self.session_epoch += 1;
                self.transcript.clear();
                self.viewer_cache = None;
                if self.layout.banner {
                    self.transcript
                        .push(Block::new(Kind::Banner, e_core::VERSION));
                }
                set_tab_title(&tab_title(&title_path(), None));
                extui::shutdown_then_start(self, "new");
            }
            "/resume" => self.open_resume_menu(),
            "/tree" => self.open_tree_menu(),
            "/settings" => self.open_settings(),
            "/copy" => self.copy_last(),
            "/undo" => self.undo_change(),
            "/usage" => self.show_usage(""),
            _ if trimmed.starts_with("/usage ") => {
                self.show_usage(trimmed["/usage ".len()..].trim())
            }
            "/fork" => self.fork_session(None),
            _ if trimmed.starts_with("/fork ") => {
                self.fork_session(Some(trimmed["/fork ".len()..].trim().to_string()))
            }
            "/export" => self.export_session(None),
            _ if trimmed.starts_with("/export ") => {
                self.export_session(Some(trimmed["/export ".len()..].trim().to_string()))
            }
            "/compact" => self.compact_now(None),
            _ if trimmed.starts_with("/compact ") => {
                self.compact_now(Some(trimmed["/compact ".len()..].trim().to_string()))
            }
            "/reload" => self.reload(),
            "/trust" => match e_core::config::trust::set(&self.agent.cwd(), true) {
                Ok(()) => {
                    self.notice(
                        "directory trusted — its AGENTS.md and .e skills/prompts now load".into(),
                    );
                    self.install_project_packages();
                }
                Err(e) => self.notice(format!("trust: {e}")),
            },
            _ if trimmed.starts_with('/') => {
                let (name, args) = trimmed[1..].split_once(' ').unwrap_or((&trimmed[1..], ""));
                if let Some(template) = e_core::resources::prompts::find(name, &self.agent.cwd()) {
                    let expanded = e_core::resources::prompts::substitute(&template.content, args);
                    self.prompt(expanded);
                } else if self.host.has_command(name) {
                    let host = self.host.clone();
                    let results = self.results.clone();
                    let (name, args) = (name.to_string(), args.to_string());
                    let epoch = self.session_epoch;
                    e_core::config::home::spawn(async move {
                        let result = host.run_command(&name, &args).await;
                        let _ = results.send(AppJob::Command { result, epoch }).await;
                    });
                } else if is_literal_slash_prompt(&trimmed) {
                    // Absolute paths and a literal leading slash are prompt
                    // text, not misspelled commands. This is how screenshot
                    // clipboard tools hand e `/var/.../capture.png question`.
                    self.prompt(trimmed);
                } else {
                    self.notice(format!("unknown command {trimmed}"));
                }
            }
            _ => self.prompt(trimmed),
        }
    }

    /// Deliver a held -r launch prompt into the current session — the pick
    /// it was waiting for fell through (declined, or the resume failed), and
    /// stranding it would silently drop typed text. The trust question, if
    /// still open, keeps holding it.
    pub(super) fn release_initial_prompt(&mut self) {
        if self.trust.is_none() {
            if let Some(initial) = self.pending_initial.take() {
                self.submit_initial(initial);
            }
        }
    }

    pub(super) fn submit_initial(&mut self, text: String) {
        if self.pending_initial_images.is_empty() {
            self.submit(text);
            return;
        }
        // Images travel with the initial launch prompt only, so this can't
        // just call submit(): the hook contract is "sees the text before
        // anything else does," and a plain submit() has no way to carry
        // images through to the eventual submission. Route the text through
        // the same hook decision submit() makes, and attach the images to
        // whatever text is actually accepted (see apply_input_verdict).
        let route = input_route(self.pending_key.is_some(), self.host.has_input_hook());
        if route == InputRoute::Hook {
            let host = self.host.clone();
            let results = self.results.clone();
            let sequence = self.input_verdicts.reserve();
            let images = std::mem::take(&mut self.pending_initial_images);
            e_core::config::home::spawn(async move {
                let verdict = host.hook_input(&text).await;
                let _ = results
                    .send(AppJob::InputVerdict {
                        sequence,
                        text,
                        images: Some(images),
                        verdict,
                    })
                    .await;
            });
            return;
        }
        let images = std::mem::take(&mut self.pending_initial_images);
        self.submit_with_images(text, images);
    }

    pub(super) fn submit_with_images(
        &mut self,
        text: String,
        images: Vec<e_core::providers::ImageInput>,
    ) {
        self.remember_prompt(text.clone());
        let count = images.len();
        let held = self.agent.submit_message(
            e_core::providers::ChatMessage::user_with_images(text.clone(), images),
            system_prompt(),
        );
        if !held {
            self.transcript.push(
                Block::new(Kind::User, display_image_prompt(&text, count)).with_images(count),
            );
        }
    }

    pub(super) fn prompt(&mut self, text: String) {
        // A fresh prompt closes the queued-prompt review and resumes the
        // queue — the reference's resume-after-new-prompt.
        self.close_queue_review();
        // While reloading or a `!` shell command is running,
        // hold the message; it submits (and displays) when the block lifts —
        // a turn must not start without the shell output it was promised.
        if self.reloading || self.shell_block.is_some() {
            self.held_prompts.push(text);
            return;
        }
        // While a turn runs the message is held and steered in (echoed later
        // via Steered); idle, it begins a turn now.
        let held = self.agent.submit(text.clone(), system_prompt());
        if !held {
            self.transcript.push(Block::new(Kind::User, text));
        }
    }
}
