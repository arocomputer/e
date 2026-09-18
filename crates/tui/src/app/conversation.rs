//! Main-screen reading position, independent of the composer and detail reader.
use super::*;
use crossterm::event::{MouseEvent, MouseEventKind};

impl App {
    /// Fixed-height views use the alternate screen; inline output keeps native history at the tail.
    pub(super) fn fixed_view(&self) -> bool {
        self.viewer.is_some()
            || self.pane.is_some()
            || self.bottom_pinned
            || self.conversation_scroll.is_some()
    }

    /// Measure the conversation column when a side pane shares the terminal.
    fn conversation_width(&self, width: usize) -> usize {
        self.pane
            .as_ref()
            .and_then(|pane| pane.split(width, &self.layout))
            .map_or(width, |(conversation, _)| conversation)
    }

    /// Scroll source rows without changing the draft. None means follow new output.
    pub(super) fn scroll_conversation(
        &mut self,
        down: bool,
        step: usize,
        width: usize,
        height: usize,
    ) {
        let width = self.conversation_width(width);
        let window = height.saturating_sub(self.composer_frame(width, height).len());
        let end = self.transcript_frame(width).len().saturating_sub(window);
        let offset = self.conversation_scroll.unwrap_or(end).min(end);
        let next = if down {
            offset.saturating_add(step).min(end)
        } else {
            offset.saturating_sub(step)
        };
        self.conversation_scroll = (next < end).then_some(next);
    }

    /// Page keys belong to the conversation only when no modal owns navigation.
    pub(super) fn conversation_key(&mut self, key: KeyEvent, width: usize, height: usize) -> bool {
        if self.menu.is_some()
            || self.settings.is_some()
            || self.auth.is_some()
            || self.trust.is_some()
            || self.ui_input_open()
            || self.pending_key.is_some()
        {
            return false;
        }
        match key.code {
            KeyCode::PageUp | KeyCode::PageDown => {
                let cols = self.conversation_width(width);
                let page = height
                    .saturating_sub(self.composer_frame(cols, height).len())
                    .max(1);
                self.scroll_conversation(key.code == KeyCode::PageDown, page, width, height);
            }
            KeyCode::End if self.conversation_scroll.is_some() => self.conversation_scroll = None,
            _ => return false,
        }
        true
    }

    /// Route wheels to the reader, pane, or conversation, never to prompt history.
    pub(super) fn mouse(&mut self, event: MouseEvent, width: usize, height: usize) {
        let down = match event.kind {
            MouseEventKind::ScrollUp => Some(false),
            MouseEventKind::ScrollDown => Some(true),
            _ => None,
        };
        if self.viewer.is_some() {
            if let Some(down) = down {
                self.scroll_viewer(down, self.scroll_lines, width, height);
            }
            return;
        }
        if self.trust.is_some()
            || self.auth.is_some()
            || self.settings.is_some()
            || self.ui_input_open()
        {
            return;
        }
        if let Some(pane) = self.pane.as_ref() {
            let in_pane = match pane.split(width, &self.layout) {
                Some((conversation, pane_width)) => match pane.side(&self.layout) {
                    e_core::config::layout::Side::Right => {
                        event.column as usize >= conversation + 3
                    }
                    e_core::config::layout::Side::Left => (event.column as usize) < pane_width,
                },
                None => pane.focused,
            };
            if in_pane {
                self.pane_mouse(event, width);
                return;
            }
        }
        if let Some(down) = down {
            self.scroll_conversation(down, self.scroll_lines, width, height);
        } else {
            self.pane_mouse(event, width);
        }
    }
}
