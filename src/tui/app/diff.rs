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
    pub(super) fn refresh_diff(&mut self) {
        if self.viewer.is_some() {
            return;
        }
        let Some(panel) = self.diff.as_mut() else {
            return;
        };
        let now = Instant::now();
        if panel.loading || (!panel.dirty && now < panel.next_refresh) {
            return;
        }
        panel.loading = true;
        panel.dirty = false;
        panel.next_refresh = now + Duration::from_millis(panel.refresh_ms);
        let generation = panel.generation;
        let cwd = self.agent.cwd().to_path_buf();
        let results = self.results.clone();
        panel.task = Some(crate::core::config::home::spawn(async move {
            let result = crate::core::diff::load_review(&cwd).await;
            let _ = results
                .send(AppJob::DiffLoaded { generation, result })
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

    /// Only pointer events inside the code pane belong to review.
    pub(super) fn diff_mouse(&mut self, mut event: crossterm::event::MouseEvent, width: usize) {
        if self.viewer.is_some()
            || self.menu.is_some()
            || self.trust.is_some()
            || self.auth.is_some()
            || self.settings.is_some()
        {
            return;
        }
        let Some(panel) = self.diff.as_mut() else {
            return;
        };
        if matches!(event.kind, crossterm::event::MouseEventKind::Up(_)) {
            let action = panel.mouse(event);
            self.diff_action(action);
            return;
        }
        if event.row as usize >= panel.height {
            return;
        }
        if let Some(left) = panel.left_width(width) {
            if (event.column as usize) < left + 2 {
                return;
            }
            event.column -= (left + 2) as u16;
        }
        let action = panel.mouse(event);
        self.menu = None;
        self.diff_action(action);
    }

    /// Keep one full-width composer below chat and code, with no vertical box rail.
    pub(super) fn frame(&mut self, width: usize, height: usize) -> Vec<String> {
        let Some(panel) = self.diff.as_ref() else {
            return self.conversation_frame(width, height);
        };
        let left_width = panel.left_width(width);
        let mut footer = self.composer_frame(width, height);
        if footer.len() > height {
            footer.drain(..footer.len() - height);
        }
        let body_height = height.saturating_sub(footer.len());
        let mut rows = if let Some(left_width) = left_width {
            let right = self
                .diff
                .as_mut()
                .map(|panel| {
                    panel.render(
                        &self.theme,
                        width.saturating_sub(left_width + 2),
                        body_height,
                    )
                })
                .unwrap_or_default();
            let mut left = self.transcript_frame(left_width);
            if left.len() > body_height {
                left.drain(..left.len() - body_height);
            }
            left.resize(body_height, String::new());
            left.into_iter()
                .zip(right)
                .map(|(left, right)| {
                    let left = crate::tui::markdown::clip_styled(&left, left_width);
                    let padding =
                        left_width.saturating_sub(crate::tui::markdown::visible_width(&left));
                    format!("{left}{}  {right}", " ".repeat(padding))
                })
                .collect::<Vec<_>>()
        } else {
            self.diff
                .as_mut()
                .map(|panel| panel.render(&self.theme, width, body_height))
                .unwrap_or_default()
        };
        rows.extend(footer);
        rows.into_iter()
            .map(|row| crate::tui::markdown::clip_styled(&row, width))
            .collect()
    }
}
