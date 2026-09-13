//! The shared frame for footer surfaces — the picker and the settings screen.
//!
//! One `─` divider above, the header, a blank, the body, one `─` divider
//! below. The hint always rides the status row, never the panel, so every
//! interface is framed identically and no surface grows a second nav line.
//! Side-by-side code review uses `review_frame` for its compact, filled pane;
//! its navigation still belongs to the shared composer status row.

use crate::tui::theme::Theme;

pub fn frame(theme: &Theme, width: usize, header: String, body: Vec<String>) -> Vec<String> {
    // The reference colours dividers with divider_style (240 dark / 250
    // light), dimmer
    // than body dim — the  token carries exactly those values.
    let divider = theme.fg("border", &"─".repeat(width));
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(divider.clone());
    out.push(header);
    out.push(String::new());
    out.extend(body);
    out.push(divider);
    out
}

/// Frame a code-review pane without a second footer or a box around the code.
/// Every row fills the pane; the caller may supply its own colored source rows.
pub fn review_frame(
    theme: &Theme,
    width: usize,
    header: String,
    body: Vec<String>,
    height: usize,
) -> Vec<String> {
    use crate::tui::markdown::{clip_styled, visible_width};
    let mut rows = vec![header];
    rows.extend(body);
    rows.resize(height, String::new());
    rows.into_iter()
        .map(|row| {
            let row = clip_styled(&row, width);
            let padding = width.saturating_sub(visible_width(&row));
            theme.bg("diffPaneBg", &format!("{row}{}", " ".repeat(padding)))
        })
        .collect()
}
