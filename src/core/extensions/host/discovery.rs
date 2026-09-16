//! Find executable extensions in home, package, and trusted project directories.

use super::*;

pub(super) fn discover() -> Vec<PathBuf> {
    let mut paths = scan(&home::extensions_dir());
    for (dir, filter) in crate::core::resources::packages::dirs("extensions") {
        // A package's filter names the top-level file or bundle directory,
        // never the entry point inside a bundle.
        paths.extend(scan(&dir).into_iter().filter(|path| {
            let top = path
                .strip_prefix(&dir)
                .ok()
                .and_then(|rel| rel.components().next())
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_default();
            filter.allows("extensions", &top)
        }));
    }
    paths
}

/// One extensions directory, sorted so launch order is stable.
pub(super) fn scan(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(entry_point) = directory_entry_point(&path) {
                paths.push(entry_point);
            }
        } else if path.is_file() && is_executable(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

/// The executable a directory extension runs. Node (and every other language's
/// relative imports) resolve against this file's own directory, so a bundled
/// `./scaffold.mjs` beside it resolves regardless of the process cwd.
pub(super) fn directory_entry_point(dir: &std::path::Path) -> Option<PathBuf> {
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let mut execs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_executable(p))
        .collect();
    // read_dir order is filesystem-dependent. Sorting makes ambiguous bundles
    // stable while preserving the documented entry-point precedence.
    execs.sort();
    let by_stem = |stem: &str| {
        execs
            .iter()
            .find(|p| p.file_stem().is_some_and(|s| s == stem))
            .cloned()
    };
    by_stem("index")
        .or_else(|| by_stem(&name))
        .or_else(|| (execs.len() == 1).then(|| execs[0].clone()))
}

#[cfg(unix)]
pub(super) fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
#[cfg(not(unix))]
pub(super) fn is_executable(_path: &std::path::Path) -> bool {
    true
}
