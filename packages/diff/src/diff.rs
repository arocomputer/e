//! Read-only workspace diffs. Git supplies tracked changes; new files are read
//! without following symlinks. Commands and output are bounded for interactive use.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt;

const MAX_OUTPUT: usize = 256 * 1024;
const MAX_FILE: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: PathBuf,
    pub added: Option<usize>,
    pub removed: Option<usize>,
    pub new: bool,
}

/// One refresh, with a patch only for the selected file.
#[derive(Debug)]
pub struct Snapshot {
    pub files: Vec<File>,
    pub selected: Option<PathBuf>,
    pub patch: String,
}

/// Run Git without a shell, pager, external diff, or index refresh writes,
/// and only ever a trusted executable: a repository must not be able to run
/// its own `git` through a relative PATH entry like `.`.
async fn git(cwd: &Path, args: &[OsString]) -> Result<(bool, Vec<u8>, bool), String> {
    let program = git_program()?;
    let mut child = tokio::process::Command::new(program)
        .args([
            "--no-pager",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-C",
        ])
        .arg(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Could not run git: {e}"))?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or("Git stdout unavailable")?
            .take(MAX_OUTPUT as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        let truncated = bytes.len() > MAX_OUTPUT;
        if truncated {
            let _ = child.kill().await;
        }
        let status = child.wait().await.map_err(|e| e.to_string())?;
        bytes.truncate(MAX_OUTPUT);
        Ok((status.success(), bytes, truncated))
    })
    .await
    .map_err(|_| "Git diff timed out".to_string())?;
    result
}

/// The Git executable, resolved once per process from absolute PATH entries
/// that live outside the workspace, falling back to the usual system
/// locations. Relative entries (`.`) and anything inside the current
/// directory are rejected: the classic workspace attack is a dropped `git`
/// plus a PATH that resolves it through the repository.
fn git_program() -> Result<PathBuf, String> {
    static GIT: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    GIT.get_or_init(|| {
        let cwd = std::env::current_dir().unwrap_or_default();
        let workspace = cwd.canonicalize().unwrap_or(cwd);
        resolve_git(std::env::var_os("PATH").as_deref(), &workspace)
            .map(Ok)
            .unwrap_or_else(|| {
                Err("no trusted git executable found on an absolute PATH entry".into())
            })
    })
    .clone()
}

/// Absolute, workspace-outside executables named `git`: the user's PATH first
/// (their chosen install wins), then the standard system locations. Public
/// and pure so the trust boundary can be tested directly.
pub fn resolve_git(path: Option<&std::ffi::OsStr>, workspace: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::env::split_paths(path.unwrap_or_default())
        .filter(|entry| entry.is_absolute())
        .map(|entry| entry.join("git"))
        .collect();
    candidates.extend(
        [
            "/usr/bin/git",
            "/usr/local/bin/git",
            "/opt/homebrew/bin/git",
        ]
        .map(PathBuf::from),
    );
    candidates.into_iter().find(|candidate| {
        let executable = |path: &Path| {
            let Ok(meta) = std::fs::metadata(path) else {
                return false;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                meta.is_file() && meta.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            meta.is_file()
        };
        executable(candidate)
            && candidate
                .canonicalize()
                .ok()
                .is_some_and(|resolved| !resolved.starts_with(workspace) && executable(&resolved))
    })
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

/// Parse NUL-delimited numstat with rename detection disabled. Paths stay bytes.
fn numstat(bytes: &[u8]) -> Result<Vec<File>, String> {
    bytes
        .split(|b| *b == 0)
        .filter(|row| !row.is_empty())
        .map(|row| {
            let mut fields = row.splitn(3, |b| *b == b'\t');
            let count = |field: Option<&[u8]>| -> Result<Option<usize>, String> {
                match field {
                    Some(b"-") => Ok(None),
                    Some(value) => std::str::from_utf8(value)
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .map(Some)
                        .ok_or("Invalid git line count".into()),
                    None => Err("Missing git line count".into()),
                }
            };
            let added = count(fields.next())?;
            let removed = count(fields.next())?;
            let path = fields.next().ok_or("Missing git path")?;
            Ok(File {
                path: PathBuf::from(OsString::from_vec(path.to_vec())),
                added,
                removed,
                new: false,
            })
        })
        .collect()
}

/// Read a new file for stats or preview. Symlinks show their target, never its contents.
async fn new_text(root: &Path, rel: &Path) -> Result<Option<String>, String> {
    let path = root.join(rel);
    let meta = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return tokio::fs::read_link(&path)
            .await
            .map(|p| Some(p.to_string_lossy().into_owned()))
            .map_err(|e| e.to_string());
    }
    if !meta.is_file() {
        return Ok(None);
    }
    let (root, rel) = (root.to_path_buf(), rel.to_path_buf());
    let file = tokio::task::spawn_blocking(move || open_in_root(&root, &rel))
        .await
        .map_err(|e| e.to_string())??;
    // fstat on the fd actually opened, not a racy path stat.
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Ok(None);
    }
    let file = tokio::fs::File::from_std(file);
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE || bytes.contains(&0) {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes).ok())
}

/// Open one file below `root` for reading without following symlinks in any
/// component between the root and the target. A final-component check alone
/// (`O_NOFOLLOW` on the last open) still lets an attacker replace an
/// intermediate directory with a symlink between listing and reading;
/// directory-FD traversal closes that window. Public for testing.
pub fn open_in_root(root: &Path, rel: &Path) -> Result<std::fs::File, String> {
    use rustix::fs::{openat, Mode, OFlags, CWD};
    if rel.is_absolute()
        || rel
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("path escapes the review root".into());
    }
    let mut dir = openat(
        CWD,
        root,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY,
        Mode::empty(),
    )
    .map_err(|e| e.to_string())?;
    let mut parts = rel.components().peekable();
    while let Some(part) = parts.next() {
        let mut flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
        if parts.peek().is_some() {
            flags |= OFlags::DIRECTORY;
        } else {
            // A directory swapped for a FIFO must not hang the read.
            flags |= OFlags::NONBLOCK;
        }
        dir = openat(&dir, part.as_os_str(), flags, Mode::empty()).map_err(|e| e.to_string())?;
    }
    Ok(std::fs::File::from(dir))
}

/// A bounded review document. Every patch keeps its original path, including
/// paths that cannot be represented as UTF-8. Omitted files are reported by the UI.
#[derive(Debug)]
pub struct Review {
    pub files: Vec<File>,
    pub patches: Vec<(PathBuf, String)>,
    pub truncated: bool,
}

/// Scan once per refresh; reuse the root and disabled filters for each patch.
async fn scan(cwd: &Path) -> Result<(PathBuf, Vec<OsString>, Vec<File>), String> {
    let (ok, mut root, _) = git(cwd, &args(&["rev-parse", "--show-toplevel"])).await?;
    if !ok {
        return Err("/diff needs a Git working tree".into());
    }
    if root.last() == Some(&b'\n') {
        root.pop();
    }
    let root = PathBuf::from(OsString::from_vec(root));
    let (has_head, _, _) = git(&root, &args(&["rev-parse", "--verify", "HEAD"])).await?;
    // Worktree comparisons also invoke clean/process filters. Override their
    // configured commands rather than letting a preview execute repository code.
    let mut diff_options = Vec::new();
    if has_head {
        let (ok, keys, truncated) =
            git(&root, &args(&["config", "--null", "--name-only", "--list"])).await?;
        if !ok || truncated {
            return Err("Could not read Git filter configuration".into());
        }
        for key in keys.split(|b| *b == 0).filter(|key| {
            key.starts_with(b"filter.")
                && (key.ends_with(b".clean")
                    || key.ends_with(b".process")
                    || key.ends_with(b".required"))
        }) {
            if key.contains(&b'=') {
                return Err("Cannot safely override a Git filter with '=' in its name".into());
            }
            let mut option = key.to_vec();
            option.extend_from_slice(if key.ends_with(b".required") {
                b"=false"
            } else {
                b"="
            });
            diff_options.push(OsString::from("-c"));
            diff_options.push(OsString::from_vec(option));
        }
    }
    let mut files = if has_head {
        let mut stat_args = diff_options.clone();
        stat_args.extend(args(&[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            "--numstat",
            "-z",
            "HEAD",
            "--",
        ]));
        let (ok, bytes, truncated) = git(&root, &stat_args).await?;
        if !ok || truncated {
            return Err("Could not load the complete changed-file list".into());
        }
        numstat(&bytes)?
    } else {
        Vec::new()
    };
    let mut list_args = args(&["ls-files", "--others", "--exclude-standard", "-z"]);
    if !has_head {
        list_args.push("--cached".into());
    }
    let (ok, bytes, truncated) = git(&root, &list_args).await?;
    if !ok || truncated {
        return Err("Could not load the complete new-file list".into());
    }
    for path in bytes.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(OsString::from_vec(path.to_vec()));
        if files.iter().any(|file| file.path == path) {
            continue;
        }
        let text = new_text(&root, &path).await.ok().flatten();
        files.push(File {
            path,
            added: text.as_ref().map(|s| s.lines().count()),
            removed: Some(0),
            new: true,
        });
        if files.len() > 2000 {
            return Err("Too many changed files to display".into());
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((root, diff_options, files))
}

/// Read one file with the same limits and filter isolation as the file-list scan.
async fn patch(root: &Path, diff_options: &[OsString], file: &File) -> Result<String, String> {
    let mut patch;
    if file.new {
        patch = match new_text(root, &file.path).await? {
            Some(text) if text.is_empty() => "Empty new file".into(),
            Some(text) => {
                let mut patch = format!("@@ -0,0 +1,{} @@\n", text.lines().count());
                for line in text.lines() {
                    patch.push('+');
                    patch.push_str(line);
                    patch.push('\n');
                    if patch.len() > MAX_OUTPUT {
                        patch.truncate(patch.floor_char_boundary(MAX_OUTPUT));
                        patch.push_str("\n… diff truncated\n");
                        break;
                    }
                }
                if !text.ends_with('\n') && patch.len() <= MAX_OUTPUT {
                    patch.push_str("\\ No newline at end of file\n");
                }
                patch
            }
            None => "Binary or large new file; preview unavailable".into(),
        };
    } else {
        let mut diff_args = diff_options.to_vec();
        diff_args.extend(args(&[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            "--unified=3",
            "HEAD",
            "--",
        ]));
        diff_args.push(file.path.as_os_str().into());
        let (ok, bytes, truncated) = git(root, &diff_args).await?;
        if !ok && !truncated {
            return Err("Could not read the selected diff".into());
        }
        patch = String::from_utf8_lossy(&bytes).into_owned();
        if truncated {
            patch.push_str("\n… diff truncated");
        }
    }
    Ok(patch)
}

/// Compare one file with HEAD, including staged and non-ignored untracked changes.
/// A repository without commits compares its current files to an empty tree.
pub async fn load(cwd: &Path, selected: Option<&Path>) -> Result<Snapshot, String> {
    let (root, options, files) = scan(cwd).await?;
    let file = selected
        .and_then(|path| files.iter().find(|file| file.path == path))
        .or_else(|| files.first());
    let (selected, patch) = if let Some(file) = file {
        (Some(file.path.clone()), patch(&root, &options, file).await?)
    } else {
        (None, String::new())
    };
    Ok(Snapshot {
        files,
        selected,
        patch,
    })
}

/// Load a continuous review without unbounded Git work or memory use. Per-file
/// failures remain visible next to their path instead of hiding the other diffs.
pub async fn load_review(cwd: &Path) -> Result<Review, String> {
    let (root, options, files) = scan(cwd).await?;
    let mut patches = Vec::new();
    let mut bytes = 0usize;
    let started = std::time::Instant::now();
    for file in &files {
        if patches.len() >= 128 || bytes >= 4 * 1024 * 1024 || started.elapsed().as_secs() >= 5 {
            break;
        }
        let source = patch(&root, &options, file)
            .await
            .unwrap_or_else(|error| error);
        if bytes.saturating_add(source.len()) > 4 * 1024 * 1024 {
            break;
        }
        bytes += source.len();
        patches.push((file.path.clone(), source));
    }
    let truncated = patches.len() < files.len();
    Ok(Review {
        files,
        patches,
        truncated,
    })
}
