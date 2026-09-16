//! e — the coding agent you can put anywhere.
//!
//! The `e` binary's own library: it re-exports the workspace crates under the
//! paths the binary, the integration tests, and the fuzz targets use, so
//! `e::core`, `e::tui`, and `e::rpc` read the same as before the split. Its
//! items are not a stable third-party API; the supported Rust surface is the
//! SDK, described in `docs/guides/extend/compatibility.md`.

pub use e_core as core;
pub use e_core::{CHANNEL, CLIENT, COMMIT, VERSION};
pub use e_rpc as rpc;
pub use e_tui as tui;
