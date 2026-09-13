//! Full transcript reader: wrapped tool output above a fixed navigation footer.
use super::*;
use crate::tui::markdown::{clip_styled, wrap_styled};

/// A reading position follows new output only while it remains at the tail.
#[derive(Clone)]
pub(super) struct Viewer {
    pub scroll: usize,
    pub follow_tail: bool,
    pub(super) hint: String,
}

impl Viewer {
    pub fn new() -> Self {
        Self {
            scroll: 0,
            follow_tail: true,
            hint: crate::core::config::settings::get_string("transcript_hint").unwrap_or_else(
                || "full detail · ctrl+o close · pgup/pgdn scroll · esc close".into(),
            ),
        }
    }
}

impl App {
    /// Rebuild the width-specific projection only when transcript or output changes.
    pub(super) fn viewer_rows(&mut self, width: usize) -> &[String] {
        let fingerprint = self.viewer_fingerprint();
        let current = self
            .viewer_cache
            .as_ref()
            .is_some_and(|(fp, cols, _)| *fp == fingerprint && *cols == width);
        if !current {
            let rows = self.project_rows(width);
            self.viewer_cache = Some((fingerprint, width, rows));
        }
        self.viewer_cache
            .as_ref()
            .map(|(_, _, rows)| rows.as_slice())
            .unwrap_or(&[])
    }

    /// Every state input used by the full transcript projection.
    fn viewer_fingerprint(&self) -> u64 {
        self.transcript.fingerprint() ^ self.output_seq.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    /// Keep the conversation's styling, with complete saved results beneath tools.
    fn project_rows(&mut self, width: usize) -> Vec<String> {
        let mut rows = Vec::new();
        for block in &mut self.transcript.blocks {
            let lines = block.review_lines(&self.theme, width);
            if lines.is_empty() {
                continue;
            }
            if !rows.is_empty() {
                rows.push(String::new());
            }
            for (row, detail) in lines {
                rows.push(clip_styled(&row, width));
                let Some(id) = detail else { continue };
                let Some(body) = Self::output_body(&self.outputs, id) else {
                    rows.extend(detail_rows(
                        &self.theme,
                        "Full saved result unavailable.",
                        width,
                    ));
                    continue;
                };
                for line in body.lines() {
                    let colored = Self::diff_row_color(&self.theme, line);
                    rows.extend(detail_rows(
                        &self.theme,
                        colored.as_deref().unwrap_or(line),
                        width,
                    ));
                }
            }
        }
        rows
    }

    /// Show the latest output on open, with fx's navigation, gap, and model row.
    pub(super) fn viewer_frame(&mut self, width: usize, height: usize) -> Vec<String> {
        let Some(viewer) = self.viewer.as_ref() else {
            return Vec::new();
        };
        let (offset, follow_tail, hint) = (viewer.scroll, viewer.follow_tail, viewer.hint.clone());
        let window = height.saturating_sub(3);
        let body = self.viewer_rows(width);
        let end = body.len().saturating_sub(window);
        let scroll = if follow_tail { end } else { offset.min(end) };
        let mut rows: Vec<String> = body.iter().skip(scroll).take(window).cloned().collect();
        rows.resize(window, String::new());
        rows.push(navigation_row(&self.theme, &hint, width));
        rows.extend(statusline(
            &self.theme,
            &self.status_data(),
            self.overlay.as_deref(),
            None,
            false,
            width,
        ));
        rows.truncate(height);
        if let Some(viewer) = self.viewer.as_mut() {
            viewer.scroll = scroll;
            viewer.follow_tail = scroll == end;
        }
        rows.into_iter()
            .map(|row| clip_styled(&row, width))
            .collect()
    }

    /// Clamp key and mouse scrolling to a full page, resuming follow at the bottom.
    pub(super) fn scroll_viewer(&mut self, down: bool, step: usize, width: usize, height: usize) {
        let end = self
            .viewer_rows(width)
            .len()
            .saturating_sub(height.saturating_sub(3));
        if let Some(viewer) = self.viewer.as_mut() {
            let offset = if viewer.follow_tail {
                end
            } else {
                viewer.scroll.min(end)
            };
            viewer.scroll = if down {
                offset.saturating_add(step).min(end)
            } else {
                offset.saturating_sub(step)
            };
            viewer.follow_tail = viewer.scroll == end;
        }
    }

    /// The reader consumes navigation, leaving global cancellation to the app.
    pub(super) fn viewer_key(&mut self, key: KeyEvent, width: usize, height: usize) -> bool {
        if self.viewer.is_none() {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Esc || (ctrl && matches!(key.code, KeyCode::Char('o' | 'c'))) {
            self.viewer = None;
            return !(ctrl && key.code == KeyCode::Char('c'));
        }
        match key.code {
            KeyCode::Up => self.scroll_viewer(false, 1, width, height),
            KeyCode::Down => self.scroll_viewer(true, 1, width, height),
            KeyCode::PageUp => {
                self.scroll_viewer(false, height.saturating_sub(3).max(1), width, height)
            }
            KeyCode::PageDown => {
                self.scroll_viewer(true, height.saturating_sub(3).max(1), width, height)
            }
            KeyCode::Home => self.scroll_viewer(false, usize::MAX, width, height),
            KeyCode::End => self.scroll_viewer(true, usize::MAX, width, height),
            _ => {}
        }
        true
    }
}

/// Tool output is literal text, not Markdown. Every physical row repeats its rail.
fn detail_rows(theme: &Theme, text: &str, width: usize) -> Vec<String> {
    let rail = theme.fg("toolDetailRail", "│");
    wrap_styled(text, width.saturating_sub(3).max(1))
        .into_iter()
        .map(|row| {
            clip_styled(
                &format!("{rail}{}", theme.fg("dim", &format!("  {row}"))),
                width,
            )
        })
        .collect()
}

/// The reference footer ellipsizes its hint instead of wrapping into the status row.
fn navigation_row(theme: &Theme, hint: &str, width: usize) -> String {
    let hint = crate::core::tools::sanitize_display(hint).replace('\n', " ");
    let available = width.saturating_sub(2);
    let hint = crate::tui::transcript::clip_plain(&hint, available);
    clip_styled(
        &format!(
            "{} {}",
            theme.fg("userMessageText", "┃"),
            theme.fg("muted", &hint)
        ),
        width,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::markdown::visible_width;

    #[test]
    fn full_detail_uses_the_reference_rail_and_wraps_every_output_row() {
        for light in [false, true] {
            let theme = crate::tui::theme::load_bundled(light).unwrap();
            assert_eq!(
                detail_rows(&theme, "alpha beta gamma delta", 16),
                vec![
                    format!("│{}", theme.fg("dim", "  alpha beta")),
                    format!("│{}", theme.fg("dim", "  gamma delta")),
                ]
            );
            let text = "界".repeat(40);
            let rows = detail_rows(&theme, &text, 16);
            assert!(rows.iter().all(|row| visible_width(row) <= 16));
            assert_eq!(
                rows.iter()
                    .map(|row| row.matches('界').count())
                    .sum::<usize>(),
                40
            );
        }
    }

    #[test]
    fn full_detail_navigation_matches_fx_and_ellipsizes_in_narrow_frames() {
        let theme = crate::tui::theme::load_bundled(false).unwrap();
        let hint = "full detail · ctrl+o close · pgup/pgdn scroll · esc close";
        assert_eq!(
            navigation_row(&theme, hint, 80),
            format!(
                "{} {}",
                theme.fg("userMessageText", "┃"),
                theme.fg("muted", hint)
            )
        );
        assert_eq!(
            navigation_row(&theme, hint, 16),
            format!(
                "{} {}",
                theme.fg("userMessageText", "┃"),
                theme.fg("muted", "full detail ·…")
            )
        );
    }
}
