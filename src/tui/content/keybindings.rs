//! The composer's line-editing keymap: which key chord performs which
//! `Key` action (`composer.rs`). File-backed like themes and skills —
//! `~/.e/keybindings.json` overrides individual chords, everything left
//! unset keeps e's built-in emacs-ish bindings.
//!
//! This covers editing only, not e's application-level shortcuts (ctrl+c,
//! ctrl+p, ctrl+v for clipboard images, tab, menu arrows, …): those are
//! claimed earlier in the key dispatch (`tui/app/mod.rs`), so a chord already
//! spoken for there never reaches this keymap regardless of what a user binds
//! it to here.
//!
//! Format: a flat JSON object, chord string to action name. The chord
//! grammar is `core/config/chord.rs`; `"none"` unbinds a default chord (the
//! event is swallowed, not passed through as a literal character).
//!
//! ```json
//! { "ctrl+j": "none", "alt+d": "kill_word" }
//! ```

use std::collections::HashMap;

use crossterm::event::KeyCode;

use crate::core::config::chord::normalize_chord;
use crate::tui::content::composer::Key;

/// A loaded set of chord overrides. Not present in the map at all means
/// "use e's built-in behavior for this chord"; present with `None` means
/// "swallow this chord, do nothing" — the two are different, so the map
/// holds `Option<Key>`, not just `Key`.
#[derive(Default)]
pub struct Keymap(HashMap<String, Option<Key>>);

impl Keymap {
    pub fn empty() -> Keymap {
        Keymap(HashMap::new())
    }

    /// `None` when `chord` has no override (fall back to the built-in
    /// mapping); `Some(None)` when it's explicitly unbound; `Some(Some(k))`
    /// for a real override.
    pub fn lookup(&self, chord: &str) -> Option<Option<Key>> {
        self.0.get(chord).copied()
    }
}

/// Load `~/.e/keybindings.json`. Missing file or malformed JSON both fail
/// open to an empty map — a keybindings typo must never make the composer
/// unusable.
pub fn load() -> Keymap {
    let Ok(json) = std::fs::read_to_string(crate::core::config::home::keybindings_path()) else {
        return Keymap::empty();
    };
    let Ok(raw) = serde_json::from_str::<HashMap<String, String>>(&json) else {
        return Keymap::empty();
    };
    let mut map = HashMap::new();
    for (chord, action) in raw {
        if action.eq_ignore_ascii_case("none") {
            map.insert(normalize_chord(&chord), None);
        } else if let Some(key) = action_of(&action) {
            map.insert(normalize_chord(&chord), Some(key));
        }
        // An unrecognized action name is dropped rather than erroring the
        // whole file: one bad entry should not cost every other override.
    }
    Keymap(map)
}

/// The base name a `KeyCode` contributes to a chord string — only the
/// bounded set this keymap can address. `None` for anything else (function
/// keys, Esc, …), which this module has no opinion on.
pub fn base_name(code: KeyCode) -> Option<String> {
    Some(
        match code {
            KeyCode::Enter => "enter",
            KeyCode::Backspace => "backspace",
            KeyCode::Delete => "delete",
            KeyCode::Left => "left",
            KeyCode::Right => "right",
            KeyCode::Up => "up",
            KeyCode::Down => "down",
            KeyCode::Home => "home",
            KeyCode::End => "end",
            KeyCode::Char(c) => return Some(c.to_string()),
            _ => return None,
        }
        .to_string(),
    )
}

/// The named actions a chord can be bound to — every `Key` variant except
/// `Char`, which is the fallback for "insert this literal character", not a
/// bindable action.
fn action_of(name: &str) -> Option<Key> {
    Some(match name {
        "enter" => Key::Enter,
        "newline" => Key::Newline,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "word_left" => Key::WordLeft,
        "word_right" => Key::WordRight,
        "home" => Key::Home,
        "end" => Key::End,
        "kill_to_end" => Key::KillToEnd,
        "kill_to_start" => Key::KillToStart,
        "kill_word" => Key::KillWord,
        _ => return None,
    })
}

// Filesystem-touching coverage (`load()` against a real keybindings.json)
// lives in `tests/keybindings.rs`, like the rest of the suite's file-backed
// config.
