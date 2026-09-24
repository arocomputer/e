//! The `~/.ulo` surface: where the home lives, the merge-write store that
//! keeps it safe, settings, per-directory trust, the chord grammar, and
//! the frame layout.

pub mod chord;
pub mod home;
pub mod layout;
pub mod settings;
pub mod store;
pub mod trust;
