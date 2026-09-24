//! Shared release discovery, verification, and resource-package installation.
//! The CLI owns executable replacement and automatic-update policy.

use std::path::Path;

/// Release assets redirect to GitHub's download hosts. This client carries
/// no provider credentials and must not be reused for authenticated requests.
fn download_client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: std::sync::OnceLock<Result<reqwest::Client, String>> =
        std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(format!("ulo/{}", crate::VERSION))
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
        return Some((base, "production", 0));
    }
    let (channel, build) = preview.split_once('-')?;
    if channel != "pr" {
        return None;
    }
    Some((base, channel, number(build)?))
}

/// Published production versions can update; preview identities stay pinned.
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

/// Discover the latest release at an API URL; an unpublished repository returns `None`.
pub async fn latest_tag_from(url: &str) -> Result<Option<String>, String> {
    let response = crate::providers::http()
        .map_err(|ulo| ulo.message)?
        .get(url)
        .header("accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|ulo| format!("update check failed: {ulo}"))?;
    if !response.status().is_success() {
        if response.status() == 404 {
            return Ok(None);
        }
        return Err(format!("update check failed: {}", response.status()));
    }
    let body: serde_json::Value = response.json().await.map_err(|ulo| ulo.to_string())?;
    body["tag_name"]
        .as_str()
        .map(|tag| Some(tag.to_string()))
        .ok_or_else(|| "release has no tag".into())
}

/// Download one asset of a release and check it against the release's
/// `checksums.txt`; a missing or mismatched sum refuses the bytes.
pub async fn fetch_verified(base: &str, tag: &str, asset: &str) -> Result<Vec<u8>, String> {
    let tarball = fetch(&format!("{base}/download/{tag}/{asset}")).await?;
    let sums = String::from_utf8(fetch(&format!("{base}/download/{tag}/checksums.txt")).await?)
        .map_err(|ulo| ulo.to_string())?;
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

/// Install a release package (`ulo install release:<owner>/<repo>/<name>`):
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
    std::fs::create_dir_all(&extensions).map_err(|ulo| ulo.to_string())?;
    let staging = root.join(format!(".staging-{tag}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|ulo| ulo.to_string())?;
    let archive = staging.join("asset.tar.gz");
    std::fs::write(&archive, &tarball).map_err(|ulo| ulo.to_string())?;
    // `xf`, not `xzf`: the system tar detects gzip itself, and a plain tar
    // published under the same name still installs.
    let unpacked = std::process::Command::new("tar")
        .arg("xf")
        .arg(&archive)
        .current_dir(&staging)
        .status()
        .map_err(|ulo| format!("tar failed: {ulo}"))?;
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
    std::fs::rename(&binary, extensions.join(name))
        .map_err(|ulo| format!("install failed: {ulo}"))?;
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::write(root.join(".tag"), tag).map_err(|ulo| ulo.to_string())?;
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
        .map_err(|ulo| format!("download failed: {ulo}"))?;
    if !response.status().is_success() {
        return Err(format!("download failed: {}", response.status()));
    }
    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|ulo| ulo.to_string())
}

/// Why an off-matrix platform cannot self-update; `ulo update` prints it.
pub const NO_RELEASE: &str =
    "no release is published for this platform — update from source, not ulo update";
