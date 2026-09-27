//! The terminal the frame loop owns: e's own input events, and the platform
//! underneath them.
//!
//! The app reads [`Event`]s and writes bytes; it never names a terminal
//! library. Native builds drive a real terminal through crossterm
//! (`native.rs`): raw mode, the input thread, signals. The browser build
//! (`web.rs`) takes its input bytes from the page, decodes them with the VT
//! parser in `vt.rs`, and writes through the sink the embedding installs.
//! The event types mirror crossterm's names so key handling reads the same
//! either way.

#[cfg(any(target_family = "wasm", test))]
pub mod vt;

#[cfg(not(target_family = "wasm"))]
mod native;
#[cfg(not(target_family = "wasm"))]
pub use native::*;

#[cfg(target_family = "wasm")]
pub mod web;
#[cfg(target_family = "wasm")]
pub use web::{
    enter, input, is_terminal, launch_row, light_background, size, write, Guard, Signals,
};

/// One thing the terminal reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// Bracketed paste: the text arrives whole, never as keys.
    Paste(String),
    /// The new size in columns and rows.
    Resize(u16, u16),
    FocusGained,
    FocusLost,
}

/// A key press, repeat, or release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
}

impl KeyEvent {
    /// A press of `code` with `modifiers` held.
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    /// Shift+Tab, as terminals report it.
    BackTab,
    Delete,
    Insert,
    /// A function key, `F(1)` through `F(24)`.
    F(u8),
    Char(char),
    Esc,
    /// A key the app has no use for (media keys, lone modifiers, …).
    Other,
}

/// Modifier keys held with a key or mouse event, as a set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = KeyModifiers(0);
    pub const SHIFT: Self = KeyModifiers(1);
    pub const ALT: Self = KeyModifiers(1 << 1);
    pub const CONTROL: Self = KeyModifiers(1 << 2);
    pub const SUPER: Self = KeyModifiers(1 << 3);
    pub const HYPER: Self = KeyModifiers(1 << 4);
    pub const META: Self = KeyModifiers(1 << 5);

    /// Every modifier in `other` is held.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Any modifier in `other` is held.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl std::ops::BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        KeyModifiers(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// A mouse report at a zero-based cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}
