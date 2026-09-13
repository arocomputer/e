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

/// Run Git without a shell, pager, external diff, or index refresh writes.
async fn git(cwd: &Path, args: &[OsString]) -> Result<(bool, Vec<u8>, bool), String> {
    let mut child = tokio::process::Command::new("git")
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
async fn new_text(path: &Path) -> Result<Option<String>, String> {
    let meta = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return tokio::fs::read_link(path)
            .await
            .map(|p| Some(p.to_string_lossy().into_owned()))
            .map_err(|e| e.to_string());
    }
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Ok(None);
    }
    let file = tokio::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .await
        .map_err(|e| e.to_string())?;
    if !file.metadata().await.map_err(|e| e.to_string())?.is_file() {
        return Ok(None);
    }
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

/// Compare the current worktree to HEAD, including staged and untracked files.
/// A repository without commits compares its current files to an empty tree.
pub async fn load(cwd: &Path, selected: Option<&Path>) -> Result<Snapshot, String> {
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
        let text = new_text(&root.join(&path)).await.ok().flatten();
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
    let file = selected
        .and_then(|path| files.iter().find(|f| f.path == path))
        .or_else(|| files.first());
    let mut patch = String::new();
    let selected = if let Some(file) = file {
        if file.new {
            patch = match new_text(&root.join(&file.path)).await? {
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
            let mut diff_args = diff_options;
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
            let (ok, bytes, truncated) = git(&root, &diff_args).await?;
            if !ok && !truncated {
                return Err("Could not read the selected diff".into());
            }
            patch = String::from_utf8_lossy(&bytes).into_owned();
            if truncated {
                patch.push_str("\n… diff truncated");
            }
        }
        Some(file.path.clone())
    } else {
        None
    };
    Ok(Snapshot {
        files,
        selected,
        patch,
    })
}
