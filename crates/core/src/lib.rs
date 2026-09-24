//! The terminal-free core of ulo: the agent turn loop, provider dialects, tools,
//! sessions, configuration, the extension host, and the embedded guides.
//!
//! The frontends are separate crates that depend on this one and never on
//! each other: `ulo_tui` (the terminal), `ulo_rpc` (`ulo rpc`), and `ulo_sdk` (the Rust
//! library). This crate names no frontend and no terminal library. Its Rust
//! items are not a stable third-party API; the supported Rust surface is the
//! SDK, described in `docs/guides/extend/compatibility.md`.
//
// Shipped code denies explicit panic sites outside test builds. Every allowed
// site needs a proof comment explaining why runtime input cannot reach it.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable
    )
)]

//! The harness — terminal-free. `agent/` is the turn loop and its satellites
//! (compaction, the system prompt); `providers/` is the wire seam, the four
//! dialects, and the catalog; `auth/` holds credentials and the sign-in
//! flows; `config/` is the ~/.ulo surface (paths, the merge-write store,
//! settings, trust); `resources/` loads skills and prompt templates; `export/`
//! renders a session branch as HTML; `extensions/`
//! is the extension host; `tools/` the built-in tools.

pub mod agent;
pub mod auth;
pub mod cli;
pub mod config;
pub mod export;
pub mod extensions;
pub mod providers;
pub mod resources;
pub mod session;
pub mod text;
pub mod tools;
pub mod update;
pub mod usage;

pub mod output;
pub mod workspace;

/// The built-in palettes, compiled in. The terminal paints with them and
/// `ulo docs theme-dark` prints them.
pub mod themes {
    /// The built-in dark theme, as JSON.
    pub const DARK: &str = include_str!("../themes/dark.json");
    /// The built-in light theme, as JSON.
    pub const LIGHT: &str = include_str!("../themes/light.json");
}

/// Release identity supplied by the release workflow; local Cargo builds keep the manifest version.
pub const VERSION: &str = env!("ULO_VERSION");
/// Update and state boundary. Local and PR builds never follow a release channel.
pub const CHANNEL: &str = env!("ULO_CHANNEL");
/// Exact source revision for published builds, or `local` for ordinary Cargo builds.
pub const COMMIT: &str = env!("ULO_COMMIT");

/// The client name ulo identifies itself with to gateways that recognize their
/// callers (sent as the provider's declared `client_header`, e.g. OpenCode's
/// `x-opencode-client`). Honest identity — ulo names itself, it does not
/// impersonate another client.
pub const CLIENT: &str = "ulo";
