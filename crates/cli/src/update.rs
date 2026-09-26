//! Application self-update policy and executable replacement.

use e_core::update::{
    fetch_verified, is_newer, is_release_version, latest_tag_from, target, NO_RELEASE,
};
use std::path::Path;

const RELEASES: &str = "https://github.com/arocomputer/e/releases";
const API_LATEST: &str = "https://api.github.com/repos/arocomputer/e/releases/latest";

/// Cargo artifacts must never be replaced by a downloaded release.
pub fn is_dev_build() -> bool {
    std::env::current_exe()
        .map(|p| p.components().any(|c| c.as_os_str() == "target"))
        .unwrap_or(true)
}

/// Respect package ownership beside the real executable, including behind symlinks.
pub fn package_update_hint(executable: &Path) -> Option<&'static str> {
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.to_owned());
    let marker = executable.parent()?.join(".e-install-method");
    match std::fs::read_to_string(marker) {
        Ok(method) => Some(match method.trim() {
            "homebrew" => "Installed with Homebrew. Update with: brew upgrade arocomputer/tap/e",
            "npm" => "Installed with npm or bun. Update with: npm install -g @arocomputer/e or bun add -g @arocomputer/e",
            _ => "This installation is package-managed. Update it with its package manager.",
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some("Cannot read installation ownership. Update with your package manager."),
    }
}

/// Local and preview builds stay pinned; production follows the latest application release.
pub async fn latest_tag() -> Result<Option<String>, String> {
    match e_core::CHANNEL {
        "production" => latest_tag_from(API_LATEST).await,
        _ => Ok(None),
    }
}

/// Verify an application archive and atomically replace the destination executable.
pub async fn install_from(base: &str, tag: &str, dest: &Path) -> Result<String, String> {
    let target = target().ok_or(NO_RELEASE)?;
    let tarball = fetch_verified(base, tag, &format!("e-{target}.tar.gz")).await?;
    let dir = dest.parent().ok_or("binary has no parent directory")?;
    let staging = dir.join(format!(".e-update-{tag}"));
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let archive = staging.join("e.tar.gz");
    std::fs::write(&archive, &tarball).map_err(|e| e.to_string())?;
    let unpacked = std::process::Command::new("tar")
        .arg("xzf")
        .arg(&archive)
        .current_dir(&staging)
        .status()
        .map_err(|e| format!("tar failed: {e}"))?;
    if !unpacked.success() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("tar failed to unpack the update".into());
    }
    let new_binary = staging.join("e");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&new_binary, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&new_binary, dest).map_err(|e| format!("install failed: {e}"))?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(tag.trim_start_matches('v').to_string())
}

/// Install a newer release only when the current executable's ownership and channel permit it.
pub async fn self_update() -> Result<Option<String>, String> {
    let dest = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(hint) = package_update_hint(&dest) {
        return Err(hint.into());
    }
    if e_core::CHANNEL != "production"
        || is_dev_build()
        || !is_release_version(e_core::VERSION)
        || target().is_none()
    {
        return Ok(None);
    }
    let Some(tag) = latest_tag().await? else {
        return Ok(None);
    };
    if !is_newer(&tag, e_core::VERSION) {
        return Ok(None);
    }
    install_from(RELEASES, &tag, &dest).await.map(Some)
}

/// Start the optional launch update and send a successful version to the terminal frontend.
pub fn background() -> Option<tokio::sync::oneshot::Receiver<String>> {
    if is_dev_build() || !e_core::config::settings::auto_update() {
        return None;
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    e_core::config::home::spawn(async move {
        if let Ok(Some(version)) = self_update().await {
            let _ = sender.send(version);
        }
    });
    Some(receiver)
}
