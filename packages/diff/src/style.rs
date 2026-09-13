//! Diff-owned colors and syntax, using the same terminal helpers as the host.
pub use crate::frame as panel;
pub use e_terminal::text::{sanitize_display, strip_ansi};
pub use e_terminal::{text, theme};

/// Resolve the extension palette, then apply the host's user-selected colors.
pub fn theme(light: bool, colors: &serde_json::Value) -> theme::Theme {
    let json = if light {
        include_str!("theme_light.json")
    } else {
        include_str!("theme_dark.json")
    };
    let mut value: serde_json::Value = serde_json::from_str(json).expect("embedded diff palette");
    if let Some(colors) = colors.as_object() {
        for (name, color) in colors {
            value["colors"][name] = color.clone();
        }
    }
    theme::Theme::from_json(&value.to_string()).expect("embedded diff palette")
}

/// Source syntax is independent of the host's Markdown palette.
pub fn highlight_diff_block(theme: &theme::Theme, lang: &str, source: &str) -> Vec<String> {
    e_terminal::highlight::highlight_block_with_tokens(
        theme,
        lang,
        source,
        [
            "diffSyntaxKeyword",
            "diffSyntaxString",
            "diffSyntaxNumber",
            "diffSyntaxComment",
            "diffSyntaxFunction",
            "diffSyntaxType",
        ],
    )
}

/// Bold is a terminal attribute, not a palette color.
pub fn bold(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[22m")
}

/// Clip a plain filename to its available display columns.
pub fn clip_plain(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= width
        })
        .collect()
}
