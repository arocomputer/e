//! Resolve the active channel's private state, with an explicit E_HOME override.
//!
//! Its formats are open conventions other tools can read: AGENTS.md, SKILL.md
//! directories, and JSONL sessions. e does not read another tool's
//! configuration or state at runtime.

use std::path::PathBuf;

#[cfg(not(target_family = "wasm"))]
tokio::task_local! {
    static SCOPED_HOME: PathBuf;
}

/// Resolve configuration for one asynchronous operation without changing
/// process environment variables. Spawned tasks must explicitly inherit it.
#[cfg(not(target_family = "wasm"))]
pub async fn scope<F: std::future::Future>(path: PathBuf, future: F) -> F::Output {
    SCOPED_HOME.scope(path, future).await
}

/// The synchronous counterpart, used by constructors and blocking log I/O.
#[cfg(not(target_family = "wasm"))]
pub fn with_home<R>(path: PathBuf, operation: impl FnOnce() -> R) -> R {
    SCOPED_HOME.sync_scope(path, operation)
}

#[cfg(not(target_family = "wasm"))]
fn scoped_home() -> Option<PathBuf> {
    SCOPED_HOME.try_with(Clone::clone).ok()
}

// The browser build has one thread and no tokio runtime, so the scope is a
// thread-local that each poll of the scoped future sets and restores — the
// same visibility a task-local gives.
#[cfg(target_family = "wasm")]
thread_local! {
    static SCOPED_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(target_family = "wasm")]
pub async fn scope<F: std::future::Future>(path: PathBuf, future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(|cx| with_home(path.clone(), || future.as_mut().poll(cx))).await
}

#[cfg(target_family = "wasm")]
pub fn with_home<R>(path: PathBuf, operation: impl FnOnce() -> R) -> R {
    let outer = SCOPED_HOME.replace(Some(path));
    let result = operation();
    SCOPED_HOME.set(outer);
    result
}

#[cfg(target_family = "wasm")]
fn scoped_home() -> Option<PathBuf> {
    SCOPED_HOME.with_borrow(Clone::clone)
}

/// Spawn a task that inherits the caller's configuration home.
pub fn spawn<F>(future: F) -> crate::rt::JoinHandle<F::Output>
where
    F: std::future::Future + crate::rt::MaybeSend + 'static,
    F::Output: crate::rt::MaybeSend + 'static,
{
    crate::rt::spawn(scope(home(), future))
}

pub fn home() -> PathBuf {
    if let Some(path) = scoped_home() {
        return path;
    }
    // An empty `E_HOME` is unset, not "the current directory": the same
    // rule `user_home` applies to `HOME`.
    if let Some(custom) = std::env::var("E_HOME").ok().filter(|h| !h.is_empty()) {
        return PathBuf::from(custom);
    }
    let base = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let directory = PathBuf::from(base).join(match crate::CHANNEL {
        "local" => ".e-dev",
        "pr" => ".e-pr",
        _ => ".e",
    });
    if crate::CHANNEL == "pr" {
        directory.join(crate::COMMIT)
    } else {
        directory
    }
}

/// A trusted workspace's own resources live in its `.e` directory.
pub fn workspace_directory(cwd: &std::path::Path) -> PathBuf {
    cwd.join(".e")
}

/// The user's home directory, when the platform declares one — the single
/// sanctioned HOME read outside [`home`] itself (the guard pins every
/// other lookup to this module). Used for `~`-relative display and the
/// trust panel's under-home ancestor.
pub fn user_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

pub fn settings_path() -> PathBuf {
    home().join("settings.json")
}
pub fn auth_path() -> PathBuf {
    home().join("auth.json")
}
pub fn agents_md_path() -> PathBuf {
    home().join("AGENTS.md")
}
pub fn sessions_dir() -> PathBuf {
    home().join("sessions")
}
pub fn extensions_dir() -> PathBuf {
    home().join("extensions")
}
pub fn skills_dir() -> PathBuf {
    home().join("skills")
}
pub fn prompts_dir() -> PathBuf {
    home().join("prompts")
}
pub fn themes_dir() -> PathBuf {
    home().join("themes")
}
/// Where `e install` clones packages: `packages/<host>/<path>` per source.
pub fn packages_dir() -> PathBuf {
    home().join("packages")
}
pub fn keybindings_path() -> PathBuf {
    home().join("keybindings.json")
}
pub fn layout_path() -> PathBuf {
    home().join("layout.json")
}

/// Create or tighten a directory owned by e, without changing its ancestors
/// or restoring owner permissions deliberately removed by the user. Existing
/// state becomes private too, including older logs below this path.
pub(crate) fn private_dir(path: &std::path::Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)?.permissions().mode() & 0o700;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

/// Make sure the home directory is private before a write lands in it, seeding
/// one bare `AGENTS.md` — the signpost for global instructions, which unlike
/// themes or skills has no command that creates it. It stays empty: anything
/// readable in it becomes system-prompt instructions (see context.rs), so
/// there is no template to ship. Everything else appears when first written,
/// so every other entry in `~/.e` is something the user (or a session) caused.
pub fn ensure() -> std::io::Result<()> {
    private_dir(&home())?;
    let agents = agents_md_path();
    if !agents.exists() {
        std::fs::File::create(&agents)?;
    }
    Ok(())
}
