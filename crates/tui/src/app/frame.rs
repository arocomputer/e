//! Compose terminal frames and route mouse input to the visible pane.

use super::*;

impl App {
    /// Color a detail-viewer row shaped like a diff row: the number-and-sign
    /// column takes the diff-marker hue (`+` green, `-` red), context and
    /// `⋯` elision rows dim, anything else passes through untouched — the
    /// reference keeps the changed text itself neutral.
    pub(super) fn diff_row_color(theme: &Theme, line: &str) -> Option<String> {
        crate::transcript::diff_row_style(theme, line)
    }

    /// Compose chat and any side pane, each with its own reading position.
    /// A terminal too narrow to split shows whichever has focus.
    pub(super) fn frame(&mut self, width: usize, height: usize) -> Vec<String> {
        self.pump_ui_queue();
        let Some(pane) = self.pane.as_ref() else {
            self.pane_hidden = false;
            return self.conversation_frame(width, height);
        };
        let split = pane.split(width, &self.layout);
        let focused = pane.focused;
        let side = pane.side(&self.layout);
        let theme = self.theme.clone();
        self.pane_hidden = split.is_none() && !focused;
        let Some((conversation_width, pane_width)) = split else {
            if focused {
                if let Some(pane) = self.pane.as_mut() {
                    return pane.render(&theme, width, height);
                }
            }
            let mut rows = self.conversation_frame(width, height);
            if rows.len() > height {
                rows.drain(..rows.len() - height);
            }
            return rows;
        };
        let pane_rows = match self.pane.as_mut() {
            Some(pane) => pane.render(&theme, pane_width, height),
            None => return self.conversation_frame(width, height),
        };
        let mut conversation = self.conversation_frame(conversation_width, height);
        if conversation.len() > height {
            conversation.drain(..conversation.len() - height);
        }
        conversation.resize(height, String::new());
        let divider = self.theme.fg("border", " │ ");
        let pad = |row: &str, to: usize| {
            let row = crate::markdown::clip_styled(row, to);
            let padding = to.saturating_sub(crate::markdown::visible_width(&row));
            format!("{row}{}", " ".repeat(padding))
        };
        conversation
            .into_iter()
            .zip(pane_rows)
            .map(|(conversation, pane)| match side {
                e_core::config::layout::Side::Right => {
                    format!("{}{divider}{pane}", pad(&conversation, conversation_width))
                }
                e_core::config::layout::Side::Left => {
                    format!("{}{divider}{conversation}", pad(&pane, pane_width))
                }
            })
            .collect()
    }

    /// The transcript and composer as one column.
    pub(super) fn conversation_frame(&mut self, width: usize, height: usize) -> Vec<String> {
        let mut lines = self.transcript_frame(width);
        let dock = self.composer_frame(width, height);
        if self.fixed_view() {
            let window = height.saturating_sub(dock.len());
            let end = lines.len().saturating_sub(window);
            let offset = self.conversation_scroll.unwrap_or(end).min(end);
            if self.conversation_scroll.is_some() {
                self.conversation_scroll = (offset < end).then_some(offset);
            }
            let mut visible: Vec<_> = lines.into_iter().skip(offset).take(window).collect();
            visible.resize(window, String::new());
            visible.extend(dock);
            if visible.len() > height {
                visible.drain(..visible.len() - height);
            }
            return visible;
        }
        lines.extend(dock);
        lines
    }

    /// Transcript and live activity only, without editor or footer chrome.
    pub(super) fn transcript_frame(&mut self, width: usize) -> Vec<String> {
        let blink_on = self
            .active
            .as_ref()
            .map(|turn| (turn.started.elapsed().as_millis() / 500) % 2 == 0)
            .unwrap_or(true);
        let mut lines = self
            .transcript
            .render_animated(&self.theme, width, blink_on);
        let dock_start = lines.len();
        let activity = self
            .ext_activity
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ");
        if let Some(s) = &self.active {
            if self.rendering_delayed {
                lines.push(String::new());
                let dot = if blink_on { "•" } else { " " };
                lines.push(
                    self.theme
                        .fg("warning", &format!("{dot} Rendering delayed")),
                );
            } else if let Some(label) = s.turn.label_with(
                s.started.elapsed().as_secs(),
                &self.layout.activity,
                &activity,
            ) {
                lines.push(String::new());
                if s.turn.recovered.is_some() {
                    // A brief, non-blinking confirmation — not an ongoing
                    // wait, so no dot animation.
                    lines.push(self.theme.fg("success", &format!("✓ {label}")));
                } else if s.turn.phase == TurnPhase::Retrying {
                    // Keep the activity row's blinking dot, toned as a
                    // warning so a struggling provider
                    // reads distinctly from ordinary thinking.
                    let dot = if blink_on { "•" } else { " " };
                    lines.push(self.theme.fg("warning", &format!("{dot} {label}")));
                } else if matches!(
                    s.turn.phase,
                    TurnPhase::Waiting
                        | TurnPhase::Thinking
                        | TurnPhase::ToolCall
                        | TurnPhase::Tool
                        | TurnPhase::AssistantText
                ) {
                    // The activity dot runs on the same column as the user
                    // rail. Once reply text is visible, the answer itself
                    // carries the turn and the label sits where the dot was.
                    if s.turn.phase == TurnPhase::AssistantText {
                        lines.push(self.theme.fg("dim", &label));
                    } else {
                        let dot = if blink_on {
                            self.theme.fg("accent", "•")
                        } else {
                            " ".to_string()
                        };
                        lines.push(format!("{dot} {}", self.theme.fg("dim", &label)));
                    }
                } else {
                    lines.push(label);
                }
            }
        }
        if self.active.is_some() {
            lines.resize(lines.len().max(dock_start + 2), String::new());
        } else if !activity.is_empty() {
            // Between turns the row is the extensions' alone.
            lines.push(String::new());
            lines.push(self.theme.fg("dim", &activity));
        }
        lines
    }

    /// One full-width editor and status band, shared beneath both review panes.
    pub(super) fn composer_frame(&mut self, width: usize, height: usize) -> Vec<String> {
        let mut lines = Vec::new();

        let entering_key = matches!(self.auth, Some(AuthStage::ApiKey { .. }));
        if !entering_key {
            // The reference caps the composer at half the frame plus one
            // row; a longer draft scrolls behind the ┃↑ marker.
            let cap = (height / 2 + 1).max(3);
            let mut composer = self.editor.render(&self.theme, width, cap);
            // Extensions' widget rows sit above everything the composer
            // owns: chrome, like the attachment labels.
            lines.extend(self.widget_rows(width));
            if !self.attachments.images.is_empty() {
                // Attachment labels are chrome, not editable prompt text.
                // The existing dim token is the palette's light gray.
                lines.push(
                    self.theme
                        .fg("dim", &image_labels(self.attachments.images.len())),
                );
            }

            // The queued banner band: the collapsed summary (ink-bright),
            // the review's hint line while it edits the queue, a gap row —
            // and with chrome above it the composer trades its leading
            // blank for its top divider, the reference's rule.
            let steering = self.agent.queued_count();
            let total = steering + self.held_prompts.len();
            if total > 0 && self.active.is_some() {
                let paused = self.queue_review.is_some();
                // Held prompts (compaction, a `!` command) can't be edited
                // into the composer — only queued steering prompts can — so
                // the affordance appears only when the edit target exists.
                let affordance = if paused || steering == 0 {
                    ""
                } else {
                    " · ↑ to edit"
                };
                let ordinary = total - steering;
                let label = if ordinary == 0 && steering == 1 {
                    format!("1 steering message{affordance}")
                } else if ordinary == 0 {
                    format!("{steering} steering messages{affordance}")
                } else if steering > 0 {
                    format!("{total} pending messages · {steering} steering{affordance}")
                } else if total == 1 {
                    format!("1 queued message{affordance}")
                } else {
                    format!("{total} queued messages{affordance}")
                };
                lines.push(self.theme.fg("userMessageText", &label));
                if let Some(review) = &self.queue_review {
                    let hint = if !review.visible {
                        "reviewing the queue · enter to apply"
                    } else if self.editor.is_empty() {
                        "delete again to remove the queued prompt · enter to send unchanged"
                    } else {
                        "enter to apply the edit"
                    };
                    lines.push(self.theme.fg("dim", hint));
                }
                lines.push(String::new());
                composer[0] = self.theme.fg("border", &"─".repeat(width));
            }
            lines.extend(composer);
        }
        if let Some(stage) = &mut self.trust {
            let dir = self.agent.cwd().to_string_lossy().into_owned();
            lines.extend(trustpanel::render_view(
                stage,
                &self.theme,
                width,
                height.saturating_sub(1),
                &dir,
            ));
        } else if let Some(stage) = &self.auth {
            lines.extend(authpanel::render(
                stage,
                &self.theme,
                width,
                self.editor.text().chars().count(),
            ));
        } else if let Some(panel) = &self.settings {
            lines.extend(panel.render(&self.theme, width));
        } else if let Some(menu) = &self.menu {
            lines.extend(menu.render(&self.theme, width));
        } else if self.ui_input_open() {
            lines.extend(self.render_ui_input(width));
        } else if let Some(panel) = &self.ext_panel {
            lines.extend(panel.render(&self.theme, width));
        }
        let ext_panel_hint = self.ext_panel.as_ref().map(|p| {
            if p.interactive {
                extui::HINT_PANEL_INTERACTIVE
            } else {
                extui::HINT_PANEL
            }
        });
        let hint = self
            .settings
            .as_ref()
            .map(|_| crate::settingspanel::HINT)
            .or_else(|| self.menu.as_ref().map(|m| m.hint))
            .or_else(|| {
                self.ui_input_open().then(|| {
                    if self.ui_editor_open() {
                        extui::HINT_EDITOR
                    } else {
                        extui::HINT_INPUT
                    }
                })
            })
            .or(ext_panel_hint)
            .map(|h| crate::menu::degrade_hint(h, width));
        // A framed surface's bottom divider sits directly above the hint
        // row — the blank spacer belongs only to the bare-composer layout.
        let panel_open = self.trust.is_some()
            || self.auth.is_some()
            || self.settings.is_some()
            || self.menu.is_some()
            || self.ui_input_open()
            || self.ext_panel.is_some();
        // The row's two sides come from the layout's templates; a transient
        // app overlay (armed exit, clipboard) takes the right side while
        // shown, and a hidden pane says how to reach it.
        let (left, right) = self.status_segments();
        let hidden_pane = self
            .pane
            .as_ref()
            .filter(|_| self.pane_hidden)
            .map(|p| format!("{} pane · {}", p.title, self.layout.focus));
        let overlay = self
            .overlay
            .clone()
            .or(self
                .attachments
                .reading
                .then(|| "reading clipboard…".to_string()))
            .or(hidden_pane)
            .or_else(|| self.conversation_scroll.map(|_| self.scroll_hint.clone()))
            .or(right);
        let footer = statusline(
            &self.theme,
            &left,
            overlay.as_deref(),
            hint,
            panel_open,
            width,
        );
        lines.extend(footer);

        lines
    }

    /// The status row's segments, left and right, from the layout's
    /// templates and everything they can name.
    pub(super) fn status_segments(&self) -> (Vec<String>, Option<String>) {
        let data = self.status_data();
        let lookup = |token: &str| -> String {
            match token {
                "model" => data
                    .model
                    .as_deref()
                    .map(e_core::output::compact_model_label)
                    .unwrap_or_default(),
                "effort" => data.effort.clone().unwrap_or_default(),
                "context" => match data.context_total.filter(|t| *t > 0) {
                    Some(total) => {
                        let percent = (data.context_used * 100) / total;
                        if percent >= 1 {
                            format!("{percent}%")
                        } else {
                            String::new()
                        }
                    }
                    None => String::new(),
                },
                "cwd" => title_path_from(
                    &self.agent.cwd(),
                    &std::env::var("HOME").unwrap_or_default(),
                ),
                "session" => self.agent.session_name().unwrap_or_default(),
                "status" => self
                    .ext_status
                    .values()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" · "),
                other => match other.strip_prefix("status:") {
                    Some(name) => self
                        .ext_status
                        .iter()
                        .filter(|(slot, _)| {
                            slot.as_str() == name
                                || slot
                                    .strip_prefix(name)
                                    .is_some_and(|rest| rest.starts_with('/'))
                        })
                        .map(|(_, text)| text.clone())
                        .collect::<Vec<_>>()
                        .join(" · "),
                    None => String::new(),
                },
            }
        };
        let expand = |templates: &[String]| -> Vec<String> {
            templates
                .iter()
                .filter_map(|t| e_core::config::layout::expand(t, &lookup))
                .collect()
        };
        let left = expand(&self.layout.status_left);
        let right = expand(&self.layout.status_right);
        let right = (!right.is_empty()).then(|| right.join(" · "));
        (left, right)
    }

    /// A mouse event while a pane is open: inside the pane it navigates,
    /// on the conversation it hands focus back.
    pub(super) fn pane_mouse(&mut self, mut event: crossterm::event::MouseEvent, width: usize) {
        if self.trust.is_some() || self.auth.is_some() || self.settings.is_some() {
            return;
        }
        let Some(pane) = self.pane.as_mut() else {
            return;
        };
        let column = event.column as usize;
        match pane.split(width, &self.layout) {
            Some((conversation_width, pane_width)) => {
                let (start, end) = match pane.side(&self.layout) {
                    e_core::config::layout::Side::Right => {
                        (conversation_width + 3, conversation_width + 3 + pane_width)
                    }
                    e_core::config::layout::Side::Left => (0, pane_width),
                };
                if column < start || column >= end {
                    if matches!(event.kind, crossterm::event::MouseEventKind::Down(_)) {
                        pane.focused = false;
                    }
                    return;
                }
                event.column = (column - start) as u16;
            }
            None if !pane.focused => return,
            None => {}
        }
        let action = pane.mouse(event);
        self.menu = None;
        self.pane_action(action);
    }

    /// Shared model and context inputs for the composer and transcript footers.
    pub(super) fn status_data(&self) -> StatusData {
        let window = self.agent.model.context_window.max(1);
        // Nothing is signed in for the current model — it's a bootstrap
        // placeholder, not something the user chose, so don't show it.
        StatusData {
            model: self.signed_in.then(|| self.agent.model_slug()),
            effort: if self.signed_in {
                self.status_effort.clone()
            } else {
                None
            },
            context_used: self.context_tokens,
            context_total: Some(window),
        }
    }

    /* ---------- pickers ---------- */
}
