//! The SDK's two error types: `Error` for everything a host can hit before
//! and around a turn, `TurnError` for a turn that ran and failed — which
//! carries the partial reply so no streamed text is lost.

use crate::Reply;

/// Anything that stops the SDK short of a running turn: an unavailable
/// model, an invalid option, a session file that cannot be opened.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The requested model is not among those ulo can serve: its provider has
    /// no credential in this home, or the name matches nothing.
    #[error("model `{0}` is unavailable — sign in to its provider (run `ulo`, then /login) or declare it in models.json")]
    ModelUnavailable(String),
    /// No model was requested and no provider is signed in, so there is no
    /// sensible default. The terminal warns and lets you sign in; an
    /// embedding has no such moment, so this is refused up front.
    #[error(
        "no provider is signed in — pass `.model()` for a declared provider, or run `ulo` and /login"
    )]
    NoProvider,
    /// The requested reasoning effort is not one the model declares.
    #[error("model `{model}` does not support effort `{effort}` (supported: {supported})")]
    Effort {
        model: String,
        effort: String,
        supported: String,
    },
    /// A `Tools::Only` entry names no built-in tool.
    #[error("unknown built-in tool `{0}`")]
    UnknownTool(String),
    /// The resolved working directory cannot be used: it does not exist, is
    /// not readable, or is not a directory. Checked in `build()` like every
    /// other up-front option.
    #[error("working directory `{}` cannot be used: {reason}", path.display())]
    Cwd {
        path: std::path::PathBuf,
        reason: String,
    },
    /// An attachment could not be read or is not an accepted image.
    #[error("{0}")]
    Image(String),
    /// A session file could not be opened, locked, or read.
    #[error("session file: {0}")]
    Session(#[from] std::io::Error),
    /// A turn ran and failed; see [`TurnError`].
    #[error(transparent)]
    Turn(#[from] TurnError),
}

/// A turn that ended in failure: a provider rejection, an exhausted retry
/// campaign, a compaction that could not be installed. `reply` holds
/// everything the turn produced before it failed — text, usage, tool
/// counts — so a host can show or log the partial result instead of
/// discarding it.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct TurnError {
    pub message: String,
    /// Boxed so `Error` variants stay small in the result-position sense
    /// clippy enforces; the reply is the one large payload a failed turn
    /// keeps, and readers deref through it transparently.
    pub reply: Box<Reply>,
}
