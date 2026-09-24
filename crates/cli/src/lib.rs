//! ulo — the coding agent you can put anywhere.
//!
//! The `ulo` binary's own library: it re-exports the workspace crates under the
//! paths the binary, the integration tests, and the fuzz targets use, so
//! `ulo::core`, `ulo::tui`, and `ulo::rpc` read the same as before the split. Its
//! items are not a stable third-party API; the supported Rust surface is the
//! SDK, described in `docs/guides/extend/compatibility.md`.

pub use ulo_core as core;
pub use ulo_core::{CHANNEL, CLIENT, COMMIT, VERSION};
pub use ulo_rpc as rpc;
pub use ulo_tui as tui;
