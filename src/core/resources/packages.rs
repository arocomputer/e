//! Packages: shareable bundles of extensions, skills, prompt templates, and
//! themes.
//!
//! A package is a git repository (or a local directory) laid out like `~/.e/`
//! itself — `extensions/`, `skills/`, `prompts/`, `themes/`, any subset, no
//! manifest. `e install <source>` clones it under
//! `~/.e/packages/<host>/<path>` and records the source string in the
//! `packages` list of `settings.json`; every loader then reads each package's
//! directory after `~/.e/`'s own, so a resource in the home shadows a
//! package's, and a trusted repo's `.e/` shadows both.
//!
//! Settings are the source of truth, not the directory: delete a clone and
//! `e install` with no arguments puts it back. Startup never touches the
//! network — a listed package missing on disk is reported in the transcript.
//! Git runs as a subprocess, so installing needs `git` on `PATH` and speaks
//! whatever protocols and credentials the user's git does.
//!
//! A release package (`release:<owner>/<repo>/<name>[@tag]`) is a compiled
//! extension published as a GitHub release asset, `<name>-<target>.tar.gz`
//! beside a `checksums.txt` — how e's own `packages/` crates reach users.
//! It installs under `~/.e/packages/releases/<owner>/<repo>/<name>` with the
//! executable in `extensions/`, the same shape as every other package.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::config::{home, settings};

/// The resource directories a package may carry, in display order.
pub const KINDS: [&str; 4] = ["extensions", "skills", "prompts", "themes"];

const SETTINGS_KEY: &str = "packages";

/// A parsed package source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A git remote, cloned under `~/.e/packages/<host>/<path>`.
    Git {
        /// The URL handed to `git clone`, ref stripped.
        url: String,
        /// Lowercased host, the first directory under the packages root.
        host: String,
        /// The repository path on that host, `.git` stripped.
        path: String,
        /// A tag, branch, or commit to pin; `None` follows the default branch.
        rev: Option<String>,
    },
    /// A directory on this machine, loaded in place — never copied.
    Local(PathBuf),
    /// A compiled extension from a GitHub release.
    Release {
        owner: String,
        repo: String,
        name: String,
        /// A release tag to pin; `None` follows the latest release.
        tag: Option<String>,
    },
}

impl Source {
    /// Parse a source string, the grammar `e install` accepts:
    ///
    /// - `git:host/user/repo[@ref]` — shorthand, cloned over HTTPS
    /// - `git:git@host:user/repo[@ref]` — scp-style SSH
    /// - `https://…`, `ssh://…`, `git://…`, `file://…` — any git URL, with
    ///   or without the `git:` prefix
    /// - `/abs/path`, `./rel`, `../rel`, `~/path` — a local directory
    pub fn parse(spec: &str) -> Result<Source, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("empty package source".into());
        }
        if spec.starts_with('-') {
            return Err(format!("`{spec}` is not a package source"));
        }
        if let Some(rest) = spec.strip_prefix("git:") {
            return parse_git(rest, spec);
        }
        if let Some(rest) = spec.strip_prefix("release:") {
            return parse_release(rest, spec);
        }
        if spec.contains("://") {
            return parse_git(spec, spec);
        }
        let local = spec.starts_with('/')
            || spec.starts_with("./")
            || spec.starts_with("../")
            || spec == "."
            || spec == ".."
            || spec.starts_with("~/");
        if local {
            return Ok(Source::Local(expand_local(spec)));
        }
        Err(format!(
            "`{spec}` is not a package source — use git:<host>/<user>/<repo>[@ref], a git URL, or a directory path"
        ))
    }

    /// Where the package's files live: the managed clone, or the local
    /// directory itself.
    pub fn root(&self) -> PathBuf {
        match self {
            Source::Git { host, path, .. } => home::packages_dir().join(host).join(path),
            Source::Local(path) => path.clone(),
            Source::Release {
                owner, repo, name, ..
            } => home::packages_dir()
                .join("releases")
                .join(owner)
                .join(repo)
                .join(name),
        }
    }

    /// Two sources name the same package when they differ only in scheme,
    /// credentials, `.git`, case of the host, or the pinned ref.
    pub fn identity(&self) -> String {
        match self {
            Source::Git { host, path, .. } => format!("{host}/{}", path.to_lowercase()),
            Source::Local(path) => path
                .canonicalize()
                .unwrap_or_else(|_| path.clone())
                .to_string_lossy()
                .into_owned(),
            Source::Release {
                owner, repo, name, ..
            } => format!(
                "release:{}/{}/{}",
                owner.to_lowercase(),
                repo.to_lowercase(),
                name
            ),
        }
    }
}

/// `release:<owner>/<repo>/<name>[@tag]`.
fn parse_release(rest: &str, spec: &str) -> Result<Source, String> {
    let (path, tag) = match rest.rsplit_once('@') {
        Some((path, tag)) if !tag.is_empty() => (path, Some(tag.to_string())),
        Some((path, _)) => (path, None),
        None => (rest, None),
    };
    let parts: Vec<&str> = path.split('/').collect();
    let [owner, repo, name] = parts.as_slice() else {
        return Err(format!(
            "`{spec}` is not a release source — use release:<owner>/<repo>/<name>[@tag]"
        ));
    };
    let clean = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            && s != "."
            && s != ".."
    };
    if !clean(owner) || !clean(repo) || !clean(name) {
        return Err(format!("`{spec}` has an unsafe release path"));
    }
    if tag.as_deref().is_some_and(|t| {
        t.starts_with('-') || t.contains(['/', '\\']) || t.contains(char::is_whitespace)
    }) {
        return Err(format!("`{spec}` has an unsafe tag"));
    }
    Ok(Source::Release {
        owner: owner.to_string(),
        repo: repo.to_string(),
        name: name.to_string(),
        tag,
    })
}

/// Install or update a release package from `base` (the releases URL) and
/// `api` (the latest-release endpoint), separated from the GitHub URLs so a
/// test can serve a release. An unpinned package that is already at the
/// latest tag is left alone.
pub async fn install_release_from(source: &Source, base: &str, api: &str) -> Result<(), String> {
    let Source::Release { name, tag, .. } = source else {
        return Ok(());
    };
    let root = source.root();
    let managed = home::packages_dir();
    if !root.starts_with(&managed) || root == managed {
        return Err(format!("refusing to write outside {}", managed.display()));
    }
    let wanted = match tag {
        Some(tag) => tag.clone(),
        None => crate::core::update::latest_tag_from(api)
            .await?
            .ok_or("the repository has no releases")?,
    };
    if crate::core::update::installed_release_tag(&root).as_deref() == Some(wanted.as_str())
        && root.join("extensions").join(name).is_file()
    {
        return Ok(());
    }
    crate::core::update::install_release_package(base, &wanted, name, &root).await
}

async fn install_release(source: &Source) -> Result<(), String> {
    let Source::Release { owner, repo, .. } = source else {
        return Ok(());
    };
    let (base, api) = crate::core::update::github_release_urls(owner, repo);
    install_release_from(source, &base, &api).await
}

fn expand_local(spec: &str) -> PathBuf {
    let path = match spec.strip_prefix("~/") {
        Some(rest) => match home::user_home() {
            Some(user_home) => user_home.join(rest),
            None => PathBuf::from(spec),
        },
        None => PathBuf::from(spec),
    };
    if path.is_absolute() {
        return path;
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(&path))
        .unwrap_or(path)
}

/// Split the trailing `@ref`, ignoring the `user@` of an SSH authority.
fn split_rev(rest: &str) -> (String, Option<String>) {
    let (scheme, remainder) = match rest.find("://") {
        Some(at) => (&rest[..at + 3], &rest[at + 3..]),
        None => ("", rest),
    };
    // A `user@` authority sits before the first `/` or `:`; a ref sits
    // after the path. Only an `@` beyond the authority is a ref marker.
    let authority_end = remainder.find(['/', ':']).unwrap_or(remainder.len());
    match remainder[authority_end..].rfind('@') {
        Some(at) => {
            let at = authority_end + at;
            let rev = remainder[at + 1..].to_string();
            let base = format!("{scheme}{}", &remainder[..at]);
            if rev.is_empty() {
                (base, None)
            } else {
                (base, Some(rev))
            }
        }
        None => (rest.to_string(), None),
    }
}

fn parse_git(rest: &str, spec: &str) -> Result<Source, String> {
    let (url, rev) = split_rev(rest.trim());
    if let Some(rev) = &rev {
        if rev.starts_with('-') || rev.chars().any(char::is_whitespace) {
            return Err(format!("`{rev}` is not a git ref"));
        }
    }
    let (host_part, path_part, url) = if let Some(at) = url.find("://") {
        // scheme://[user@]host[:port]/path
        let after = &url[at + 3..];
        let slash = after.find('/').unwrap_or(after.len());
        let authority = &after[..slash];
        let host = authority.rsplit('@').next().unwrap_or(authority);
        let host = host.split(':').next().unwrap_or(host);
        (host.to_string(), after[slash..].to_string(), url.clone())
    } else if let Some((authority, path)) = url.split_once(':') {
        // scp-style: [user@]host:path
        if authority.contains('/') {
            return Err(format!("`{spec}` is not a git source"));
        }
        let host = authority.rsplit('@').next().unwrap_or(authority);
        (host.to_string(), path.to_string(), url.clone())
    } else {
        // shorthand host/user/repo
        match url.split_once('/') {
            Some((host, path)) => (
                host.to_string(),
                path.to_string(),
                format!("https://{host}/{path}"),
            ),
            None => return Err(format!("`{spec}` is not a git source")),
        }
    };
    let host = host_part.trim().to_lowercase();
    if host.is_empty() && !url.starts_with("file://") {
        return Err(format!("`{spec}` has no host"));
    }
    let host = if host.is_empty() {
        "file".to_string()
    } else {
        host
    };
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(format!("`{host}` is not a host name"));
    }
    let path = path_part.trim_matches('/');
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    if path.is_empty() {
        return Err(format!("`{spec}` names no repository"));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return Err(format!("`{spec}` has an unsafe repository path"));
        }
    }
    Ok(Source::Git {
        url,
        host,
        path: path.to_string(),
        rev,
    })
}

/// A configured package, as `e packages` shows it.
pub struct Package {
    /// The settings entry, verbatim.
    pub spec: String,
    pub status: Status,
}

pub enum Status {
    /// On disk; per-kind resource counts in [`KINDS`] order.
    Installed { root: PathBuf, counts: [usize; 4] },
    /// Listed in settings, absent on disk — `e install` restores it.
    Missing,
    /// The settings entry does not parse.
    Invalid(String),
}

/// The sources recorded in `settings.json`, in order — what `e install` and
/// `e remove` edit.
pub fn settings_entries() -> Vec<String> {
    settings::get_strings(SETTINGS_KEY).unwrap_or_default()
}

/// A trusted repository's own list: `<cwd>/.e/packages`, one source per
/// line, `#` comments. Shared by the team through the repository; installs
/// land in the user's managed roots like any other package.
pub fn project_entries(cwd: &Path) -> Vec<String> {
    if !crate::core::config::trust::trusted(cwd) {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(cwd.join(".e").join("packages")) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Every configured source: settings first, then the current directory's
/// project list (entries already in settings are not repeated).
pub fn configured() -> Vec<String> {
    let mut entries = settings_entries();
    let cwd = std::env::current_dir().unwrap_or_default();
    for entry in project_entries(&cwd) {
        let same = |a: &str, b: &str| match (Source::parse(a), Source::parse(b)) {
            (Ok(a), Ok(b)) => a.identity() == b.identity(),
            _ => a == b,
        };
        if !entries.iter().any(|known| same(known, &entry)) {
            entries.push(entry);
        }
    }
    entries
}

/// Roots loaded for this process only (`--package`), kept beside the
/// configured ones and forgotten at exit.
fn once_roots() -> &'static std::sync::Mutex<Vec<PathBuf>> {
    static ROOTS: std::sync::OnceLock<std::sync::Mutex<Vec<PathBuf>>> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Load a package for this run without recording it: a local directory is
/// used in place, a git source is cloned into a temporary directory, a
/// release asset is fetched into one. Returns the root.
pub async fn use_once(spec: &str) -> Result<PathBuf, String> {
    let source = Source::parse(spec)?;
    let root = match &source {
        Source::Local(path) => {
            if !path.is_dir() {
                return Err(format!("{} is not a directory", path.display()));
            }
            path.clone()
        }
        Source::Git { url, rev, .. } => {
            let dir = std::env::temp_dir().join(format!(
                "e-package-{}-{}",
                std::process::id(),
                once_roots().lock().unwrap_or_else(|e| e.into_inner()).len()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            git(
                None,
                &["clone", "--quiet", "--", url, &dir.to_string_lossy()],
            )?;
            if let Some(rev) = rev {
                checkout(&dir, rev)?;
            }
            dir
        }
        Source::Release {
            owner,
            repo,
            name,
            tag,
            ..
        } => {
            let dir = std::env::temp_dir().join(format!(
                "e-package-{}-{}",
                std::process::id(),
                once_roots().lock().unwrap_or_else(|e| e.into_inner()).len()
            ));
            let (base, api) = crate::core::update::github_release_urls(owner, repo);
            let wanted = match tag {
                Some(tag) => tag.clone(),
                None => crate::core::update::latest_tag_from(&api)
                    .await?
                    .ok_or("the repository has no releases")?,
            };
            crate::core::update::install_release_package(&base, &wanted, name, &dir).await?;
            dir
        }
    };
    once_roots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(root.clone());
    Ok(root)
}

/// Remove the temporary clones `use_once` made. Local directories are
/// untouched.
pub fn forget_once() {
    let roots = std::mem::take(&mut *once_roots().lock().unwrap_or_else(|e| e.into_inner()));
    let prefix = format!("e-package-{}-", std::process::id());
    for root in roots {
        let temporary = root
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
            && root.starts_with(std::env::temp_dir());
        if temporary {
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}

/// Every configured package with its on-disk status.
pub fn list() -> Vec<Package> {
    configured()
        .into_iter()
        .map(|spec| {
            let status = match Source::parse(&spec) {
                Err(reason) => Status::Invalid(reason),
                Ok(source) => {
                    let root = source.root();
                    if root.is_dir() {
                        Status::Installed {
                            counts: counts(&root),
                            root,
                        }
                    } else {
                        Status::Missing
                    }
                }
            };
            Package { spec, status }
        })
        .collect()
}

/// The roots of every package present on disk, in settings order, then
/// the project's, then this run's `--package` roots.
pub fn roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = configured()
        .iter()
        .filter_map(|spec| Source::parse(spec).ok())
        .map(|source| source.root())
        .filter(|root| root.is_dir())
        .collect();
    for root in once_roots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
    {
        if !roots.contains(root) {
            roots.push(root.clone());
        }
    }
    roots
}

/// Each installed package's `<kind>/` directory, when it has one.
pub fn dirs(kind: &str) -> Vec<PathBuf> {
    roots()
        .into_iter()
        .map(|root| root.join(kind))
        .filter(|dir| dir.is_dir())
        .collect()
}

/// Settings entries that are not on disk, for the startup notice.
pub fn missing() -> Vec<String> {
    list()
        .into_iter()
        .filter(|p| matches!(p.status, Status::Missing))
        .map(|p| p.spec)
        .collect()
}

/// How many resources a package root carries of each kind, in [`KINDS`]
/// order: executables (or bundle directories), `SKILL.md` folders, `.md`
/// files, `.json` files.
pub fn counts(root: &Path) -> [usize; 4] {
    let entries = |kind: &str| -> Vec<PathBuf> {
        std::fs::read_dir(root.join(kind))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    };
    let extensions = entries("extensions")
        .iter()
        .filter(|p| p.is_dir() || is_executable(p))
        .count();
    let skills = entries("skills")
        .iter()
        .filter(|p| p.join("SKILL.md").is_file())
        .count();
    let has_ext = |p: &PathBuf, ext: &str| p.extension().is_some_and(|x| x == ext);
    let prompts = entries("prompts")
        .iter()
        .filter(|p| has_ext(p, "md"))
        .count();
    let themes = entries("themes")
        .iter()
        .filter(|p| has_ext(p, "json"))
        .count();
    [extensions, skills, prompts, themes]
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Install one source: clone (or sync an existing clone to its ref) and
/// record it in settings, replacing an entry for the same package at another
/// ref. Returns the root and its resource counts.
pub async fn install(spec: &str) -> Result<(PathBuf, [usize; 4]), String> {
    let source = Source::parse(spec)?;
    match &source {
        Source::Local(path) => {
            if !path.is_dir() {
                return Err(format!("{} is not a directory", path.display()));
            }
        }
        Source::Git { .. } => sync(&source)?,
        Source::Release { .. } => install_release(&source).await?,
    }
    let root = source.root();
    if counts(&root).iter().all(|n| *n == 0) {
        eprintln!(
            "note: {} carries no extensions/, skills/, prompts/, or themes/",
            root.display()
        );
    }
    record(&source, spec.trim()).map_err(|e| format!("could not update settings.json: {e}"))?;
    let counts = counts(&root);
    Ok((root, counts))
}

/// Make disk match settings: clone what is missing, sync every git package
/// to its ref (or the tip of its default branch when unpinned). Returns one
/// line per package for the report; the first failure stops nothing else.
pub async fn install_all() -> Vec<Result<String, String>> {
    let mut results = Vec::new();
    for spec in configured() {
        results.push(install_one(&spec).await);
    }
    results
}

async fn install_one(spec: &str) -> Result<String, String> {
    let source = Source::parse(spec)?;
    match &source {
        Source::Local(path) if !path.is_dir() => {
            Err(format!("{spec}: {} is not a directory", path.display()))
        }
        Source::Local(_) => Ok(format!("{spec}: in place")),
        Source::Git { .. } => {
            let fresh = !source.root().is_dir();
            sync(&source).map_err(|e| format!("{spec}: {e}"))?;
            Ok(format!(
                "{spec}: {}",
                if fresh { "installed" } else { "up to date" }
            ))
        }
        Source::Release { .. } => {
            let before = crate::core::update::installed_release_tag(&source.root());
            install_release(&source)
                .await
                .map_err(|e| format!("{spec}: {e}"))?;
            let after = crate::core::update::installed_release_tag(&source.root());
            Ok(format!(
                "{spec}: {}",
                match (before, after) {
                    (None, Some(tag)) => format!("installed {tag}"),
                    (Some(old), Some(new)) if old != new => format!("updated {old} → {new}"),
                    _ => "up to date".to_string(),
                }
            ))
        }
    }
}

/// Forget a package: drop its settings entry and delete a managed clone. A
/// local directory is left alone — e never owned it.
pub fn remove(spec: &str) -> Result<PathBuf, String> {
    let source = Source::parse(spec)?;
    let identity = source.identity();
    let mut entries = settings_entries();
    let before = entries.len();
    entries.retain(|entry| {
        Source::parse(entry)
            .map(|s| s.identity() != identity)
            .unwrap_or(true)
    });
    if entries.len() == before {
        return Err(format!("{spec} is not installed"));
    }
    settings::set_strings(SETTINGS_KEY, &entries)
        .map_err(|e| format!("could not update settings.json: {e}"))?;
    let root = source.root();
    if matches!(source, Source::Git { .. } | Source::Release { .. }) {
        let managed = home::packages_dir();
        if root.starts_with(&managed) && root != managed && root.exists() {
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("could not delete {}: {e}", root.display()))?;
            // Empty `<host>/<user>` parents are litter, not state.
            let mut parent = root.parent();
            while let Some(dir) = parent {
                if dir == managed || std::fs::remove_dir(dir).is_err() {
                    break;
                }
                parent = dir.parent();
            }
        }
    }
    Ok(root)
}

/// Append the source as typed, replacing any entry for the same package so
/// `e install …@v2` moves a pin instead of duplicating it.
fn record(source: &Source, spec: &str) -> std::io::Result<()> {
    let identity = source.identity();
    let mut entries = settings_entries();
    entries.retain(|entry| {
        Source::parse(entry)
            .map(|s| s.identity() != identity)
            .unwrap_or(true)
    });
    entries.push(spec.to_string());
    settings::set_strings(SETTINGS_KEY, &entries)
}

/// Bring a git package's clone to the requested state: a fresh clone when
/// absent, otherwise fetch and check out the pinned ref, or fast-forward the
/// default branch. A failed fresh clone leaves nothing behind.
fn sync(source: &Source) -> Result<(), String> {
    let Source::Git { url, rev, .. } = source else {
        return Ok(());
    };
    let root = source.root();
    let managed = home::packages_dir();
    if !root.starts_with(&managed) || root == managed {
        return Err(format!("refusing to write outside {}", managed.display()));
    }
    if !root.is_dir() {
        if let Some(parent) = root.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let result = git(
            None,
            &["clone", "--quiet", "--", url, &root.to_string_lossy()],
        )
        .and_then(|_| match rev {
            Some(rev) => checkout(&root, rev),
            None => Ok(()),
        });
        if let Err(e) = result {
            let _ = std::fs::remove_dir_all(&root);
            return Err(e);
        }
        return Ok(());
    }
    git(Some(&root), &["fetch", "--quiet", "--tags", "origin"])?;
    match rev {
        Some(rev) => checkout(&root, rev),
        None => {
            // A clone that was pinned earlier sits detached; return to the
            // remote's default branch before fast-forwarding.
            if git(Some(&root), &["symbolic-ref", "--quiet", "HEAD"]).is_err() {
                let head = git(
                    Some(&root),
                    &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
                )?;
                let branch = head
                    .trim()
                    .strip_prefix("origin/")
                    .unwrap_or(head.trim())
                    .to_string();
                // A branch name is a ref, not a pathspec, so no `--` here;
                // git itself refuses to create names that start with `-`.
                git(Some(&root), &["checkout", "--quiet", &branch])?;
            }
            git(Some(&root), &["pull", "--quiet", "--ff-only"]).map(|_| ())
        }
    }
}

/// Detach at `rev`: the remote branch of that name first (so a branch pin
/// tracks the fetched tip, not a stale local branch), then the tag or commit.
fn checkout(root: &Path, rev: &str) -> Result<(), String> {
    let remote = format!("origin/{rev}");
    if git(
        Some(root),
        &["checkout", "--quiet", "--detach", &remote, "--"],
    )
    .is_ok()
    {
        return Ok(());
    }
    git(Some(root), &["checkout", "--quiet", "--detach", rev, "--"]).map(|_| ())
}

/// Run git, returning stdout; a failure carries git's own stderr.
fn git(cwd: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .args(args)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    Err(if stderr.is_empty() {
        format!("git {} failed", args.first().unwrap_or(&""))
    } else {
        format!("git {}: {stderr}", args.first().unwrap_or(&""))
    })
}

/// True when a path sits inside the managed packages root — the skills
/// picker uses it to label a skill's scope.
pub fn is_packaged(path: &Path) -> bool {
    path.starts_with(home::packages_dir()) || roots().iter().any(|root| path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_parts(spec: &str) -> (String, String, String, Option<String>) {
        match Source::parse(spec).unwrap() {
            Source::Git {
                url,
                host,
                path,
                rev,
            } => (url, host, path, rev),
            Source::Local(_) | Source::Release { .. } => panic!("{spec} parsed as another kind"),
        }
    }

    #[test]
    fn shorthand_clones_over_https_and_splits_the_ref() {
        let (url, host, path, rev) = git_parts("git:github.com/intuitums/e-diff@v1");
        assert_eq!(url, "https://github.com/intuitums/e-diff");
        assert_eq!(host, "github.com");
        assert_eq!(path, "intuitums/e-diff");
        assert_eq!(rev.as_deref(), Some("v1"));
        let (_, _, _, rev) = git_parts("git:github.com/intuitums/e-diff");
        assert_eq!(rev, None);
    }

    #[test]
    fn urls_and_scp_forms_share_one_identity() {
        let specs = [
            "git:github.com/Intuitums/e-diff@v1",
            "https://github.com/intuitums/e-diff.git",
            "git:git@github.com:intuitums/e-diff@main",
            "ssh://git@github.com/intuitums/e-diff",
            "git:ssh://git@github.com:22/intuitums/e-diff@release/1.0",
        ];
        let identities: std::collections::HashSet<String> = specs
            .iter()
            .map(|s| Source::parse(s).unwrap().identity())
            .collect();
        assert_eq!(identities.len(), 1, "{identities:?}");
        let (url, _, _, rev) = git_parts("git:git@github.com:intuitums/e-diff@main");
        assert_eq!(url, "git@github.com:intuitums/e-diff");
        assert_eq!(rev.as_deref(), Some("main"));
        let (_, _, _, rev) = git_parts("git:ssh://git@github.com:22/intuitums/e-diff@release/1.0");
        assert_eq!(rev.as_deref(), Some("release/1.0"));
    }

    #[test]
    fn managed_root_is_host_then_path_and_never_escapes() {
        let source = Source::parse("git:github.com/intuitums/e-diff").unwrap();
        assert_eq!(
            source.root(),
            home::packages_dir()
                .join("github.com")
                .join("intuitums/e-diff")
        );
        assert!(Source::parse("git:github.com/../x").is_err());
        assert!(Source::parse("git:github.com/intuitums/e@-bad").is_err());
        assert!(Source::parse("--upload-pack=x").is_err());
        assert!(Source::parse("git:github.com").is_err());
    }

    #[test]
    fn local_paths_load_in_place_and_bare_words_are_refused() {
        assert!(matches!(
            Source::parse("/tmp/pkg").unwrap(),
            Source::Local(_)
        ));
        assert!(matches!(Source::parse("./pkg").unwrap(), Source::Local(p) if p.is_absolute()));
        assert!(Source::parse("intuitums/e-diff").is_err());
        let (_, host, path, _) = git_parts("file:///tmp/pkg");
        assert_eq!((host.as_str(), path.as_str()), ("file", "tmp/pkg"));
    }
}
