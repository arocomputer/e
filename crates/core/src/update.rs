//! Self-update: fetch the latest release binary for this platform, verify
//! its checksum, and swap it in place — `e update` runs it by hand, and the
//! TUI runs it in the background at launch (opt out with the Auto-update
//! setting). The swap is an atomic rename next to the running binary; the
//! new version takes effect on the next start, which the notice says.
//!
//! Local Cargo builds are exempt: a binary living under a `target/` directory is a
//! cargo artifact, and auto-update must never stomp one. So is any platform
//! off the release matrix (`target()` is `None`): a `cargo install` on musl,
//! armv7, FreeBSD, … must never be overwritten with a tarball its host
//! cannot run. Package-managed installs carry an ownership marker beside the
//! executable; both manual and automatic updates stop before any network request.

use std::path::Path;

const RELEASES: &str = "https://github.com/intuitums/e/releases";
const BETA_RELEASES: &str = "https://github.com/intuitums/e-beta/releases";
const BETA_API_LATEST: &str = "https://api.github.com/repos/intuitums/e-beta/releases/latest";
const DEV_UPDATE: &str =
    "Dev builds use npm install -g @intuitums/e@dev or bun add -g @intuitums/e@dev";
const API_LATEST: &str = "https://api.github.com/repos/intuitums/e/releases/latest";

/// Release assets redirect to GitHub's download hosts. This client carries
/// no provider credentials and must not be reused for authenticated requests.
fn download_client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: std::sync::OnceLock<Result<reqwest::Client, String>> =
        std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(format!("e/{}", crate::VERSION))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    let downgrade = attempt.previous().last().is_some_and(|previous| {
                        previous.scheme() == "https" && attempt.url().scheme() != "https"
                    });
                    if downgrade || attempt.previous().len() >= 10 {
                        attempt.error("unsafe or excessive release redirects")
                    } else {
                        attempt.follow()
                    }
                }))
                .connect_timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// The release artifact name for this build's platform — `None` when the
/// release matrix (`.github/workflows/release.yml`) ships nothing for it, so
/// no update path may guess a tarball.
pub fn target() -> Option<&'static str> {
    release_target(
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(target_env = "gnu"),
    )
}

/// `target()` as a pure function of the platform, so the off-matrix case
/// can be pinned from a machine that is on it. Linux releases are glibc
/// builds; a musl (Alpine) install is off the matrix.
pub fn release_target(os: &str, arch: &str, gnu_libc: bool) -> Option<&'static str> {
    Some(match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") if gnu_libc => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") if gnu_libc => "x86_64-unknown-linux-gnu",
        _ => return None,
    })
}

/// True when the running binary is a cargo build, not an installed release.
pub fn is_dev_build() -> bool {
    std::env::current_exe()
        .map(|p| p.components().any(|c| c.as_os_str() == "target"))
        .unwrap_or(true)
}

/// Package installers leave ownership beside the real executable, including behind symlinks.
/// A present but unreadable or unknown marker still prevents self-update.
pub fn package_update_hint(executable: &Path) -> Option<&'static str> {
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.to_owned());
    let marker = executable.parent()?.join(".e-install-method");
    match std::fs::read_to_string(marker) {
        Ok(method) => Some(match method.trim() {
            "homebrew-beta" => "Installed with Homebrew. Update with: brew upgrade intuitums/tap/e-beta",
            "homebrew-dev" => DEV_UPDATE,
            "npm-beta" => "Update with: npm install -g @intuitums/e@beta or bun add -g @intuitums/e@beta",
            "npm-dev" => "Update with: npm install -g @intuitums/e@dev or bun add -g @intuitums/e@dev",
            "homebrew" => "Installed with Homebrew. Update with: brew upgrade intuitums/tap/e",
            "npm" => "Installed with npm or bun. Update with: npm install -g @intuitums/e or bun add -g @intuitums/e",
            _ => "This installation is package-managed. Update it with its package manager.",
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some("Cannot read installation ownership. Update with your package manager."),
    }
}

/// Parse supported release identities without crossing channels or accepting arbitrary tags.
fn release_parts(version: &str) -> Option<([u64; 3], &str, u64)> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let (base, preview) = version.split_once('-').unwrap_or((version, ""));
    let values: Vec<_> = base.split('.').collect();
    if values.len() != 3 {
        return None;
    }
    let number = |s: &str| -> Option<u64> {
        if s.is_empty()
            || (s.len() > 1 && s.starts_with('0'))
            || !s.bytes().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        s.parse().ok()
    };
    let base = [number(values[0])?, number(values[1])?, number(values[2])?];
    if preview.is_empty() {
        return Some((base, "stable", 0));
    }
    let parts: Vec<_> = preview.split('.').collect();
    if parts.len() != 3 || !["dev", "beta", "pr"].contains(&parts[0]) {
        return None;
    }
    let hash = parts[2].strip_prefix('g')?;
    if hash.len() != 12 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((base, parts[0], number(parts[1])?))
}

/// Published stable, dev, and beta versions can update; PR identities stay pinned.
pub fn is_release_version(v: &str) -> bool {
    release_parts(v).is_some_and(|(_, channel, _)| channel != "pr")
}

/// Compare only releases in the same channel, never switching a user's installation.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (release_parts(candidate), release_parts(current)) {
        (Some((a, ac, an)), Some((b, bc, bn))) if ac == bc && ac != "pr" => (a, an) > (b, bn),
        _ => false,
    }
}

/// The latest release tag ("v0.4.1"), from the GitHub API.
/// `None` — no release published — means nothing to update to, which the
/// flow reads as already current, not a failure.
pub async fn latest_tag() -> Result<Option<String>, String> {
    match crate::CHANNEL {
        "stable" => latest_tag_from(API_LATEST).await,
        "beta" => latest_tag_from(BETA_API_LATEST).await,
        "dev" => Err(DEV_UPDATE.into()),
        _ => Ok(None),
    }
}

/// `latest_tag` against an explicit API URL, so tests can serve the API.
pub async fn latest_tag_from(url: &str) -> Result<Option<String>, String> {
    let response = crate::providers::http()
        .map_err(|e| e.message)?
        .get(url)
        .header("accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("update check failed: {e}"))?;
    if !response.status().is_success() {
        if response.status() == 404 {
            return Ok(None);
        }
        return Err(format!("update check failed: {}", response.status()));
    }
    let body: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    body["tag_name"]
        .as_str()
        .map(|tag| Some(tag.to_string()))
        .ok_or_else(|| "release has no tag".into())
}

/// Download `tag` for this platform from `base`, verify its checksum, and
/// atomically replace `dest`. Returns the installed version. `base` is a
/// parameter so tests can serve a fake release.
pub async fn install_from(base: &str, tag: &str, dest: &Path) -> Result<String, String> {
    let target = target().ok_or(NO_RELEASE)?;
    let tarball = fetch_verified(base, tag, &format!("e-{target}.tar.gz")).await?;

    // Unpack next to the destination so the final rename stays on one
    // filesystem; the system tar does the extraction (no archive deps).
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

/// Download one asset of a release and check it against the release's
/// `checksums.txt`; a missing or mismatched sum refuses the bytes.
async fn fetch_verified(base: &str, tag: &str, asset: &str) -> Result<Vec<u8>, String> {
    let tarball = fetch(&format!("{base}/download/{tag}/{asset}")).await?;
    let sums = String::from_utf8(fetch(&format!("{base}/download/{tag}/checksums.txt")).await?)
        .map_err(|e| e.to_string())?;
    let expected = sums
        .lines()
        .find(|l| l.ends_with(&format!(" {asset}")))
        .and_then(|l| l.split_whitespace().next())
        .ok_or("no checksum for this platform")?;
    let actual = {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&tarball);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    if actual != expected {
        return Err("checksum mismatch — refusing to install".into());
    }
    Ok(tarball)
}

/// The release base URL and latest-release API URL for a GitHub repository.
pub fn github_release_urls(owner: &str, repo: &str) -> (String, String) {
    (
        format!("https://github.com/{owner}/{repo}/releases"),
        format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"),
    )
}

/// Install a release package (`e install release:<owner>/<repo>/<name>`):
/// fetch `<name>-<target>.tar.gz` for this platform from `base` at `tag`,
/// verify it, and place the `<name>` executable at
/// `<root>/extensions/<name>`, where the extension host finds it. `<root>/.tag`
/// records what is installed so an unpinned package can follow releases.
pub async fn install_release_package(
    base: &str,
    tag: &str,
    name: &str,
    root: &Path,
) -> Result<(), String> {
    let target = target().ok_or(NO_RELEASE)?;
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(format!("`{name}` is not a release asset name"));
    }
    let tarball = fetch_verified(base, tag, &format!("{name}-{target}.tar.gz")).await?;
    let extensions = root.join("extensions");
    std::fs::create_dir_all(&extensions).map_err(|e| e.to_string())?;
    let staging = root.join(format!(".staging-{tag}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let archive = staging.join("asset.tar.gz");
    std::fs::write(&archive, &tarball).map_err(|e| e.to_string())?;
    // `xf`, not `xzf`: the system tar detects gzip itself, and a plain tar
    // published under the same name still installs.
    let unpacked = std::process::Command::new("tar")
        .arg("xf")
        .arg(&archive)
        .current_dir(&staging)
        .status()
        .map_err(|e| format!("tar failed: {e}"))?;
    if !unpacked.success() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("tar failed to unpack the release asset".into());
    }
    let binary = staging.join(name);
    if !binary.is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("the release asset does not contain `{name}`"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&binary, extensions.join(name)).map_err(|e| format!("install failed: {e}"))?;
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::write(root.join(".tag"), tag).map_err(|e| e.to_string())?;
    Ok(())
}

/// The tag a release package was installed at, from its `.tag` file.
pub fn installed_release_tag(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join(".tag"))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let response = download_client()?
        .get(url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("download failed: {}", response.status()));
    }
    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string())
}

/// Why an off-matrix platform cannot self-update; `e update` prints it.
pub const NO_RELEASE: &str =
    "no release is published for this platform — update from source, not e update";

/// The whole flow for the running binary: check, install if newer. Ok(None)
/// means already current (or not applicable).
pub async fn self_update() -> Result<Option<String>, String> {
    let dest = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(hint) = package_update_hint(&dest) {
        return Err(hint.into());
    }
    // Local and PR builds stay pinned. Published builds follow only their
    // own channel, and unsupported platforms never download another target.
    if !["stable", "dev", "beta"].contains(&crate::CHANNEL)
        || is_dev_build()
        || !is_release_version(crate::VERSION)
        || target().is_none()
    {
        return Ok(None);
    }
    // No published release: nothing exists to update to — already current.
    let Some(tag) = latest_tag().await? else {
        return Ok(None);
    };
    if !is_newer(&tag, crate::VERSION) {
        return Ok(None);
    }
    let base = if crate::CHANNEL == "beta" {
        BETA_RELEASES
    } else {
        RELEASES
    };
    install_from(base, &tag, &dest).await.map(Some)
}
