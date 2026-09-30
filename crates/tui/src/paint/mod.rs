//! Paint primitives: SGR, the diffing screen, theme, background detection,
//! and code highlighting.

#[cfg(not(target_family = "wasm"))]
pub mod background;
pub mod highlight;
pub mod render;
pub mod screen;
pub mod theme;

mod wove;
