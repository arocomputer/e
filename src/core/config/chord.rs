//! The chord grammar: one canonical spelling for a key chord, shared by
//! everything in `~/.e` that names keys — `keybindings.json`
//! (`tui/content/keybindings.rs`), the layout's focus chord
//! (`config/layout.rs`), and extension shortcuts (`extensions/host.rs`).
//! Terminal-free on purpose: it turns strings into strings, so the core can
//! validate a chord without knowing how a terminal reports one.
//!
//! A chord is `[ctrl+][alt+][shift+]<key>`, modifiers in any order,
//! case-insensitive; `<key>` is `enter`, `backspace`, `delete`, `left`,
//! `right`, `up`, `down`, `home`, `end`, or a single character — including
//! `+` and `-` themselves (`ctrl+-`), since modifiers are read off the front
//! and whatever remains is the key. A capital letter is spelled with its
//! modifier (`shift+a`), which is how the terminal reports it.

/// Build a canonical chord string from modifiers and a base name — the same
/// function both a live `KeyEvent` and a parsed JSON key are run through, so
/// the two always compare equal for the same physical chord. The base is
/// lowercased here, on both sides: a typed capital arrives as `Char('A')`
/// plus SHIFT, and must meet the file's `shift+a`.
pub fn chord_string(ctrl: bool, alt: bool, shift: bool, base: &str) -> String {
    let mut s = String::new();
    if ctrl {
        s.push_str("ctrl+");
    }
    if alt {
        s.push_str("alt+");
    }
    if shift {
        s.push_str("shift+");
    }
    s.push_str(&base.to_ascii_lowercase());
    s
}

/// Parse a user-written chord string ("shift+ctrl+A", "Alt+J", "ctrl-w")
/// into the same canonical form `chord_string` produces, so modifier order
/// and case in the file never matter. Modifiers are peeled off the front one
/// `name+` (or `name-`) at a time and the remainder is the key verbatim,
/// which is what lets `ctrl+-` and `ctrl++` name the `-` and `+` keys.
pub fn normalize_chord(raw: &str) -> String {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let lower = raw.trim().to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some((head, tail)) = rest.split_once(['+', '-']) {
        match head.trim() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" | "meta" => alt = true,
            "shift" => shift = true,
            _ => break,
        }
        rest = tail.trim_start();
    }
    chord_string(ctrl, alt, shift, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_string_and_normalize_chord_agree_regardless_of_order_or_case() {
        assert_eq!(
            chord_string(true, false, false, "a"),
            normalize_chord("Ctrl+A")
        );
        assert_eq!(
            chord_string(true, true, false, "left"),
            normalize_chord("alt+ctrl+left")
        );
        assert_eq!(
            chord_string(false, true, true, "enter"),
            normalize_chord("SHIFT+ALT+ENTER")
        );
    }

    /// The live side sees a capital as `Char('A')` + SHIFT and must meet the
    /// file's `shift+a`; and `-`/`+` are keys, not just separators.
    #[test]
    fn shifted_letters_and_separator_keys_match_the_live_chord() {
        assert_eq!(
            chord_string(false, false, true, "A"),
            normalize_chord("shift+a")
        );
        assert_eq!(chord_string(true, false, false, "-"), "ctrl+-");
        assert_eq!(normalize_chord("ctrl+-"), "ctrl+-");
        assert_eq!(normalize_chord("ctrl++"), "ctrl++");
        assert_eq!(normalize_chord("ctrl-w"), "ctrl+w");
    }
}
