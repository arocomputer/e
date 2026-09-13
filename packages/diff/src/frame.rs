//! Filled rows for the review document.
use e_terminal::theme::Theme;
/// Frame a code-review pane without a second footer or a box around the code.
/// Every row fills the pane; the caller may supply its own colored source rows.
pub fn review_frame(
    theme: &Theme,
    width: usize,
    header: String,
    body: Vec<String>,
    height: usize,
) -> Vec<String> {
    use e_terminal::text::{clip_styled, visible_width};
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
