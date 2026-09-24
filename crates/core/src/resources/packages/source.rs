//! Parse npm, git, local, and release sources and resolve their install locations.

use super::*;

/// A parsed package source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// An npm package, installed under `~/.ulo/packages/npm/node_modules/<name>`.
    Npm {
        /// The package name, `@scope/name` included.
        name: String,
        /// A version, range, or dist-tag to pin; `None` follows `latest`.
        version: Option<String>,
    },
    /// A git remote, cloned under `~/.ulo/packages/<host>/<path>`.
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
    /// Parse a source string, the grammar `ulo install` accepts:
    ///
    /// - `npm:name[@version]`, `npm:@scope/name[@version]` — from the
    ///   user's npm registry
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
        if let Some(rest) = spec.strip_prefix("npm:") {
            return parse_npm(rest, spec);
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
            "`{spec}` is not a package source — use npm:<name>[@version], git:<host>/<user>/<repo>[@ref], a git URL, or a directory path"
        ))
    }

    /// Where the package's files live: the managed clone, or the local
    /// directory itself.
    pub fn root(&self) -> PathBuf {
        match self {
            Source::Npm { name, .. } => npm_prefix().join("node_modules").join(name),
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
            Source::Npm { name, .. } => format!("npm:{}", name.to_lowercase()),
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

/// `npm:[@scope/]name[@version]`. Names follow npm's rules closely enough
/// to be safe as a directory and as an argument: lowercase, URL-safe
/// characters, no leading dot or dash, one optional `@scope/`.
pub(super) fn parse_npm(rest: &str, spec: &str) -> Result<Source, String> {
    let rest = rest.trim();
    let (name, version) = match rest.strip_prefix('@') {
        // A scoped name has its own leading `@`; the version's comes after.
        Some(scoped) => match scoped.split_once('@') {
            Some((name, version)) => (format!("@{name}"), Some(version)),
            None => (format!("@{scoped}"), None),
        },
        None => match rest.split_once('@') {
            Some((name, version)) => (name.to_string(), Some(version)),
            None => (rest.to_string(), None),
        },
    };
    let bare = name.strip_prefix('@').unwrap_or(&name);
    let segments: Vec<&str> = bare.split('/').collect();
    let valid_segment = |s: &str| {
        !s.is_empty()
            && s.len() <= 214
            && !s.starts_with(['.', '-', '_'])
            && s.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.')
            })
    };
    let shape_ok = match (name.starts_with('@'), segments.as_slice()) {
        (false, [only]) => valid_segment(only),
        (true, [scope, pkg]) => valid_segment(scope) && valid_segment(pkg),
        _ => false,
    };
    if !shape_ok {
        return Err(format!("`{spec}` is not an npm package name"));
    }
    let version = match version {
        Some("") => return Err(format!("`{spec}` has an empty version")),
        Some(v) if v.starts_with('-') || v.chars().any(char::is_whitespace) => {
            return Err(format!("`{spec}` has an unsafe version"))
        }
        Some(v) => Some(v.to_string()),
        None => None,
    };
    Ok(Source::Npm { name, version })
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
fn expand_local(spec: &str) -> PathBuf {
    let path = match spec.strip_prefix("~/") {
        Some(rest) => match home::user_home() {
            Some(user_home) => user_home.join(rest),
            None => PathBuf::from(spec),
        },
        None => PathBuf::from(spec),
    };
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    };
    // The real path, so `./pkg` and a symlinked temp root record the same
    // way they resolve; a path that does not exist yet stays as written.
    path.canonicalize().unwrap_or(path)
}

/// Split the trailing `@ref`, ignoring the `user@` of an SSH authority.
fn split_rev(rest: &str) -> (String, Option<String>) {
    let (scheme, remainder) = match rest.find("://") {
        Some(at) => (&rest[..at + 3], &rest[at + 3..]),
        None => ("", rest),
    };
    // A `user@` (or `user:password@`) authority sits before the path; a ref
    // sits after it. Only an `@` beyond the authority is a ref marker. With
    // a scheme the authority runs to the first `/` — a `:` inside it is a
    // port or a password. Without one (scp form) it ends at the first `:`.
    let authority_end = if scheme.is_empty() {
        remainder.find(['/', ':'])
    } else {
        remainder.find('/')
    }
    .unwrap_or(remainder.len());
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
    // The host becomes the first directory under the managed root, so it
    // must be a plain name: dots inside are fine, a leading one is not.
    if host.starts_with('.')
        || !host
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
