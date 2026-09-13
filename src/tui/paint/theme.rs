//! File-backed theme selection for the terminal frontend.
pub use e_terminal::theme::Theme;

/// The two palettes are compiled into the binary — no runtime files, no
/// themes directory. The raw JSON is exposed so tests can assert on it.
pub const LIGHT_JSON: &str = include_str!("theme_light.json");
pub const DARK_JSON: &str = include_str!("theme_dark.json");

pub fn bundled_json(light: bool) -> &'static str {
    if light {
        LIGHT_JSON
    } else {
        DARK_JSON
    }
}

/// Load one of the two bundled palettes.
pub fn load_bundled(light: bool) -> Result<Theme, String> {
    Theme::from_json(bundled_json(light))
}

/// A user theme from `~/.e/themes/<name>.json`, if present and valid.
pub fn load_user(name: &str) -> Option<Theme> {
    let path = crate::core::config::home::themes_dir().join(format!("{name}.json"));
    let json = std::fs::read_to_string(path).ok()?;
    Theme::from_json(&json).ok()
}

/// Resolve the effective theme for a selection and a detected background.
/// `~/.e/themes/<name>.json` wins over the built-ins for any name — so even
/// `light`/`dark` are overridable — falling back to the embedded pair.
pub fn resolve(selection: &str, detected_light: bool) -> Theme {
    let name = if selection == "auto" {
        if detected_light {
            "light"
        } else {
            "dark"
        }
    } else {
        selection
    };
    if let Some(theme) = load_user(name) {
        return theme;
    }
    let light = name == "light" || (name != "dark" && detected_light);
    // The dark theme is embedded in this binary; if it failed to parse,
    // that is a build bug CI catches, not a runtime state. Scoped allow,
    // proof: compile-time data.
    #[allow(clippy::expect_used)]
    load_bundled(light).unwrap_or_else(|_| load_bundled(false).expect("embedded dark"))
}
