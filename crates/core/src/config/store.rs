//! The non-destructive store: writes preserve unknown keys, quarantine
//! corrupt files, and never wipe on a parse error.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::config::home;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One lock for every store write. The read-modify-write in [`update`] must
/// not interleave — two racing writers would each rename a snapshot over the
/// other's key. The mutex handles threads; the file lock handles independent
/// ulo processes. Every store shares one lock because all stores live in the
/// same small home and writes are rare.
static WRITE_LOCK: Mutex<()> = Mutex::new(());
/// Recoverable load problems collected before the TUI exists to display them.
static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Drain configuration warnings for the frontend. A corrupt file is already
/// safe at the returned backup path; this makes that recovery visible.
pub fn take_warnings() -> Vec<String> {
    std::mem::take(&mut *WARNINGS.lock().unwrap_or_else(|ulo| ulo.into_inner()))
}

struct WriteGuard {
    _thread: MutexGuard<'static, ()>,
    _process: File,
}

fn lock_write() -> io::Result<WriteGuard> {
    // A panicked writer only poisons its own snapshot; the next writer starts
    // from disk anyway.
    let thread = WRITE_LOCK.lock().unwrap_or_else(|ulo| ulo.into_inner());
    home::ensure()?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let process = options.open(home::home().join(".store.lock"))?;
    process.lock()?;
    Ok(WriteGuard {
        _thread: thread,
        _process: process,
    })
}

/// Read a JSON object from `path`. A file that exists but won't parse is
/// quarantined aside as `<name>.corrupt-<ms>` (never clobbered) and an empty
/// object returned; a genuinely absent file reads as empty. Any *other*
/// read error — permissions, I/O — aborts: writing on top of it would erase
/// data ulo can't see.
pub fn read_object(path: &Path) -> io::Result<Map<String, Value>> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => quarantine(path, "expected a JSON object"),
            Err(error) => quarantine(path, &error.to_string()),
        },
        Err(ulo) if ulo.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(ulo) => Err(ulo),
    }
}

/// Move a corrupt file aside so the next write starts clean while the user's
/// bytes stay recoverable. Failing to preserve the original aborts the caller.
fn quarantine(path: &Path, reason: &str) -> io::Result<Map<String, Value>> {
    let aside = path.with_extension(format!("corrupt-{}", now_ms()));
    std::fs::rename(path, &aside)?;
    WARNINGS
        .lock()
        .unwrap_or_else(|ulo| ulo.into_inner())
        .push(format!(
            "invalid configuration file {}: {reason}; moved it to {} and using defaults",
            path.display(),
            aside.display()
        ));
    Ok(Map::new())
}

/// Merge changes into the file, preserving every other key. `mutate` sees the
/// current on-disk object and edits it in place. Written atomically.
///
/// Errors are loud: an unreadable source file (`PermissionDenied`, …) or a
/// failed preservation of corrupt bytes returns without touching anything.
pub fn update<F: FnOnce(&mut Map<String, Value>)>(
    path: &Path,
    mode: u32,
    mutate: F,
) -> io::Result<()> {
    update_inner(path, mode, None, mutate)
}

/// Update a versioned object without allowing an older ulo to rewrite a format
/// it does not understand. Unversioned and older objects remain writable so
/// the next successful write upgrades them in place.
pub fn update_versioned<F: FnOnce(&mut Map<String, Value>)>(
    path: &Path,
    mode: u32,
    supported: u32,
    mutate: F,
) -> io::Result<()> {
    update_inner(path, mode, Some(supported), mutate)
}

fn update_inner<F: FnOnce(&mut Map<String, Value>)>(
    path: &Path,
    mode: u32,
    supported: Option<u32>,
    mutate: F,
) -> io::Result<()> {
    let _guard = lock_write()?;
    let mut object = read_object(path)?;
    if let (Some(supported), Some(version)) = (supported, object.get("format_version")) {
        match version.as_u64() {
            Some(version) if version <= u64::from(supported) => {}
            Some(version) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "configuration format {version} is newer than this ulo supports ({supported})"
                    ),
                ));
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "configuration format_version must be a non-negative integer",
                ));
            }
        }
    }
    mutate(&mut object);
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(object))?
    );
    write_atomic(path, &text, mode)
}

fn write_atomic(path: &Path, contents: &str, mode: u32) -> io::Result<()> {
    // Unique per attempt: stale files from a killed writer must never be
    // reused. The process lock prevents live writers from racing, while
    // create_new handles PID reuse and leftovers without clobbering them.
    static ATTEMPT: AtomicU64 = AtomicU64::new(0);
    let (tmp, mut file) = loop {
        let n = ATTEMPT.fetch_add(1, Ordering::Relaxed);
        let candidate = path.with_extension(format!("tmp-{}-{n}", std::process::id()));
        match create_staging(&candidate) {
            Ok(file) => break (candidate, file),
            Err(ulo) if ulo.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(ulo) => return Err(ulo),
        }
    };
    if let Err(error) = file.write_all(contents.as_bytes()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(mode)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(error);
        }
    }
    drop(file);
    std::fs::rename(&tmp, path).inspect_err(|_| {
        // Don't leave the temp behind if the destination cannot be replaced.
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Create staging files privately, before any credential bytes are written.
fn create_staging(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn staging_is_private_before_the_first_write() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("ulo-private-stage-{}", uuid::Uuid::new_v4()));
        let file = super::create_staging(&path).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o077, 0);
        assert_eq!(file.metadata().unwrap().len(), 0);
        assert_eq!(
            super::create_staging(&path).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
