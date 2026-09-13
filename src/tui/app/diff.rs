//! Diff-panel lifecycle, async Git refreshes, and composer attachments.
use super::*;
use crate::tui::diffpanel::{Action, DiffPanel};

impl App {
    /// Toggle review without starting a model turn or changing repository state.
    pub(super) fn toggle_diff(&mut self) {
        if self.diff.take().is_none() {
            if self.trust.is_some() {
                return;
            }
            self.diff_generation += 1;
            self.diff = Some(DiffPanel::new(self.diff_generation));
        }
    }

    /// At most one bounded Git read is in flight while the panel is visible.
    pub(super) fn refresh_diff(&mut self, width: usize) {
        if self.viewer.is_some() {
            return;
        }
        let Some(panel) = self.diff.as_mut() else {
            return;
        };
        if !panel.focused && panel.left_width(width).is_none() {
            return;
        }
        let now = Instant::now();
        if panel.loading || (!panel.dirty && now < panel.next_refresh) {
            return;
        }
        panel.loading = true;
        panel.dirty = false;
        panel.next_refresh = now + Duration::from_millis(panel.refresh_ms);
        let requested = panel.selected.clone();
        let generation = panel.generation;
        let cwd = self.agent.cwd().to_path_buf();
        let results = self.results.clone();
        panel.task = Some(crate::core::config::home::spawn(async move {
            let result = crate::core::diff::load(&cwd, requested.as_deref()).await;
            let _ = results
                .send(AppJob::DiffLoaded {
                    generation,
                    requested,
                    result,
                })
                .await;
        }));
    }

    pub(super) fn diff_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Close => {
                self.diff = None;
            }
            Action::Attach { label, content } => {
                self.editor
                    .insert_attachment(&label, &format!("\n{content}\n"));
            }
        }
    }

    /// Panel focus never steals keys from a dialog or global cancellation.
    pub(super) fn diff_key(&mut self, key: KeyEvent) -> bool {
        if self.viewer.is_some()
            || self.menu.is_some()
            || self.trust.is_some()
            || self.auth.is_some()
            || self.settings.is_some()
        {
            return false;
        }
        let Some(panel) = self.diff.as_mut() else {
            return false;
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    panel.focused = !panel.focused;
                    return true;
                }
                KeyCode::Char('c') => {
                    self.diff = None;
                    return false;
                }
                KeyCode::Char('o') => return false,
                _ => {}
            }
        }
        if !panel.focused {
            return false;
        }
        let action = panel.key(key);
        self.diff_action(action);
        true
    }

    pub(super) fn diff_mouse(&mut self, mut event: crossterm::event::MouseEvent, width: usize) {
        if self.viewer.is_some()
            || self.trust.is_some()
            || self.auth.is_some()
            || self.settings.is_some()
        {
            return;
        }
        let Some(panel) = self.diff.as_mut() else {
            return;
        };
        if let Some(left) = panel.left_width(width) {
            if (event.column as usize) < left + 3 {
                if matches!(event.kind, crossterm::event::MouseEventKind::Down(_)) {
                    panel.focused = false;
                }
                return;
            }
            event.column -= (left + 3) as u16;
        } else if !panel.focused {
            return;
        }
        let action = panel.mouse(event);
        self.menu = None;
        self.diff_action(action);
    }

    /// Compose a fixed-height split so patch scrolling never moves the conversation.
    pub(super) fn frame(&mut self, width: usize, height: usize) -> Vec<String> {
        let Some(panel) = self.diff.as_mut() else {
            return self.conversation_frame(width, height);
        };
        let Some(left_width) = panel.left_width(width) else {
            return if panel.focused {
                panel.render(&self.theme, width, height)
            } else {
                let mut rows = self.conversation_frame(width, height);
                if rows.len() > height {
                    rows.drain(..rows.len() - height);
                }
                rows
            };
        };
        let right_width = width.saturating_sub(left_width + 3);
        let right = panel.render(&self.theme, right_width, height);
        let mut left = self.conversation_frame(left_width, height);
        if left.len() > height {
            left.drain(..left.len() - height);
        }
        left.resize(height, String::new());
        let divider = self.theme.fg("border", " │ ");
        left.into_iter()
            .zip(right)
            .map(|(left, right)| {
                let left = crate::tui::markdown::clip_styled(&left, left_width);
                let padding = left_width.saturating_sub(crate::tui::markdown::visible_width(&left));
                format!("{left}{}{divider}{right}", " ".repeat(padding))
            })
            .collect()
    }
}
