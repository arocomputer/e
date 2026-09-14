//! Theme loading: `themes/{light,dark}.json` → a resolved palette.
//!
//! The files carry a `vars` block with grayscale xterm-256 values and hex diff
//! colors. The `colors` block maps tokens to a var name, a direct value, or
//! `""` for the terminal default. The parity
//! tests pin both the var values and that light/dark are structural mirrors.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct ThemeFile {
    vars: HashMap<String, serde_json::Value>,
    colors: HashMap<String, serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct Theme {
    /// token → SGR foreground prefix ("" tokens map to the default-fg reset).
    fg: HashMap<String, String>,
    /// raw var values, kept for tests and lightness inference.
    pub vars: HashMap<String, i64>,
    colors: HashMap<String, serde_json::Value>,
}

fn fg_code(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(n) => n
            .as_u64()
            .filter(|n| *n <= 255)
            .map(|n| format!("\x1b[38;5;{n}m")),
        serde_json::Value::String(s) if s.is_empty() => Some("\x1b[39m".to_string()),
        serde_json::Value::String(s) if s.starts_with('#') => {
            // Exactly six ASCII hex digits after '#'; anything else is a
            // user-edit typo and must fall back, not panic on a slice.
            let hex = &s[1..];
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let (r, g, b) = (
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            );
            Some(format!("\x1b[38;2;{r};{g};{b}m"))
        }
        _ => None,
    }
}

impl Theme {
    /// Resolved color values for another process; never raw terminal sequences.
    pub fn colors(&self) -> &HashMap<String, serde_json::Value> {
        &self.colors
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let file: ThemeFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut vars = HashMap::new();
        for (name, value) in &file.vars {
            if let Some(n) = value.as_i64() {
                vars.insert(name.clone(), n);
            }
        }
        let mut fg = HashMap::new();
        let mut colors = HashMap::new();
        for (token, value) in &file.colors {
            // A color is a var reference, or a literal (number / hex / "").
            let resolved = match value {
                serde_json::Value::String(s) if file.vars.contains_key(s.as_str()) => {
                    fg_code(&file.vars[s.as_str()])
                }
                other => fg_code(other),
            };
            if let Some(code) = resolved {
                let value = match value.as_str().and_then(|name| file.vars.get(name)) {
                    Some(value) => value,
                    None => value,
                };
                colors.insert(token.clone(), value.clone());
                fg.insert(token.clone(), code);
            }
        }
        Ok(Theme { fg, vars, colors })
    }

    /// Wrap text in the token's foreground, closing with the default-fg reset.
    pub fn fg(&self, token: &str, text: &str) -> String {
        match self.fg.get(token) {
            Some(code) if code != "\x1b[39m" => format!("{code}{text}\x1b[39m"),
            _ => text.to_string(),
        }
    }

    /// The raw SGR prefix for a token ("" for default).
    pub fn fg_prefix(&self, token: &str) -> &str {
        match self.fg.get(token) {
            Some(code) if code != "\x1b[39m" => code,
            _ => "",
        }
    }

    /// The token's value as a background prefix ("" for default). The theme
    /// files carry one value per token; whether it paints ink or ground is
    /// the caller's choice — the reference's filled selection rows use the
    /// same palette entries as backgrounds.
    pub fn bg_prefix(&self, token: &str) -> String {
        match self.fg.get(token) {
            Some(code) if code != "\x1b[39m" => code.replacen("\x1b[38;", "\x1b[48;", 1),
            _ => String::new(),
        }
    }

    /// Fill a row or span with a theme background, then restore the terminal background.
    pub fn bg(&self, token: &str, text: &str) -> String {
        let prefix = self.bg_prefix(token);
        format!("{prefix}{text}\x1b[49m")
    }

    /// The diff-marker token for one side of a diff: the truecolor value when
    /// the terminal advertises 24-bit color, the reference's 256-color
    /// fallback otherwise.
    pub fn diff_marker_token(added: bool) -> &'static str {
        let truecolor = std::env::var("COLORTERM")
            .map(|v| v.contains("truecolor") || v.contains("24bit"))
            .unwrap_or(false);
        match (added, truecolor) {
            (true, true) => "toolDiffAddedMarker",
            (true, false) => "toolDiffAddedMarkerFallback",
            (false, true) => "toolDiffRemovedMarker",
            (false, false) => "toolDiffRemovedMarkerFallback",
        }
    }
}
