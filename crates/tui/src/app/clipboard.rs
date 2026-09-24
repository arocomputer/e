//! Read image attachments or text from the desktop clipboard without a resident helper.
//!
//! Every subprocess run is bounded twice — a wall-clock deadline and a
//! stdout cap — so a hung or flooding clipboard owner (or a PATH-replaced
//! helper) can neither stall a ctrl+v forever nor balloon memory: the run
//! is killed and the read surfaces as a notice.

use ulo_core::providers::{ImageInput, MAX_IMAGE_BYTES};

/// One clipboard read may take this long before its helpers are killed.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
/// Bounded helpers are looked up at their system paths, not via `PATH`.
#[cfg(target_os = "macos")]
const OSASCRIPT: &str = "/usr/bin/osascript";
#[cfg(target_os = "macos")]
const SIPS: &str = "/usr/bin/sips";

/// Why a bounded helper run produced nothing usable: the helper is not
/// installed (`Missing` — try the next helper), or it failed, timed out, or
/// overran the cap (`Failed` — give up, the payload is unusable either way).
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) enum RunError {
    Missing,
    Failed(String),
}

/// A finished helper run: its exit status and its capped stdout.
struct RunOutput {
    success: bool,
    stdout: Vec<u8>,
}

/// Run a helper, bounded by [`READ_TIMEOUT`] and [`MAX_IMAGE_BYTES`]. Every
/// exit path kills the helper's process group and reaps the helper.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run(program: &str, args: &[&str]) -> Result<RunOutput, RunError> {
    let mut child = spawn_grouped(program, args)?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(RunError::Failed(format!("{program}: no stdout pipe")));
    };
    let reader = Reader::spawn(stdout);
    let stdout = match reader.output.recv_timeout(READ_TIMEOUT) {
        Ok(buffer) => buffer,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            return Err(give_up(&mut child, &reader, program));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Vec::new(),
    };
    if stdout.len() as u64 > MAX_IMAGE_BYTES {
        // The reader stopped draining, so the child may be blocked on a
        // full pipe and never exit on its own — kill the group first.
        ulo_core::tools::kill_group(child.id());
        let _ = child.wait();
        let _ = reader.thread.join();
        return Err(RunError::Failed(format!(
            "{program} output exceeded the {} MiB image limit",
            MAX_IMAGE_BYTES / (1024 * 1024)
        )));
    }
    let Some(success) = wait_for_exit(&mut child) else {
        return Err(give_up(&mut child, &reader, program));
    };
    // The leader has exited, but a forked descendant may still hold the
    // pipe and block the reader — take the group down before waiting for
    // the reader's exit.
    ulo_core::tools::kill_group(child.id());
    let _ = child.wait();
    await_reader(&reader.done);
    let _ = reader.thread.join();
    Ok(RunOutput { success, stdout })
}

/// Start the helper with stdout piped, in its own process group, so the
/// kill in [`run`] reaches any forked descendant still holding the pipe
/// (the bash tool's pattern).
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn spawn_grouped(program: &str, args: &[&str]) -> Result<std::process::Child, RunError> {
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command, Stdio};
    Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RunError::Missing
            } else {
                RunError::Failed(format!("{program}: {error}"))
            }
        })
}

/// Give the helper a bounded chance to exit on its own: its success, or
/// None when it outlived the deadline. The caller kills the whole group
/// while the child is unreaped either way — its pid is still the valid
/// group id then, and cannot yet have been reused.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn wait_for_exit(child: &mut std::process::Child) -> Option<bool> {
    let deadline = std::time::Instant::now() + READ_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.success()),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return None,
        }
    }
}

/// The helper ran past its deadline: kill its group, reap it, and report
/// the timeout.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn give_up(child: &mut std::process::Child, reader: &Reader, program: &str) -> RunError {
    ulo_core::tools::kill_group(child.id());
    let _ = child.wait();
    await_reader(&reader.done);
    RunError::Failed(format!(
        "{program} did not finish within {}s",
        READ_TIMEOUT.as_secs()
    ))
}

/// Read (bounded) on a helper thread so the deadline can fire even when
/// the child never closes its pipe; the pipe dies with the group kill.
/// A second channel carries the reader's exit, so a pathological process
/// that escaped the group and still holds the pipe can only ever cost a
/// bounded wait — never a hang, and never a bricked clipboard.
#[cfg(any(target_os = "macos", target_os = "linux"))]
struct Reader {
    /// What was read, once the pipe closed or the cap was passed.
    output: std::sync::mpsc::Receiver<Vec<u8>>,
    /// Signalled when the thread is about to exit.
    done: std::sync::mpsc::Receiver<()>,
    thread: std::thread::JoinHandle<()>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Reader {
    /// Drain `stdout` until it closes or passes [`MAX_IMAGE_BYTES`].
    fn spawn(mut stdout: std::process::ChildStdout) -> Self {
        use std::io::Read as _;
        let (sender, output) = std::sync::mpsc::channel();
        let (done_sender, done) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        buffer.extend_from_slice(&chunk[..read]);
                        if buffer.len() as u64 > MAX_IMAGE_BYTES {
                            break;
                        }
                    }
                }
            }
            let _ = sender.send(buffer);
            let _ = done_sender.send(());
        });
        Self {
            output,
            done,
            thread,
        }
    }
}

/// Wait briefly for the reader thread to finish. A process that escaped
/// the group and still holds the pipe would block a plain join forever;
/// after the bounded wait the thread is left to die whenever the pipe
/// finally closes, and the read reports its result regardless.
fn await_reader(done: &std::sync::mpsc::Receiver<()>) {
    let _ = done.recv_timeout(std::time::Duration::from_secs(2));
}

/// Clipboard content accepted by the composer. Images remain real attachment
/// data rather than editable placeholder text.
pub(super) enum Paste {
    Images(Vec<ImageInput>),
    Text(String),
}

/// Read the richest supported clipboard representation. A copied image wins;
/// when there is no image, ctrl+v can still act as a text paste.
pub(super) fn read() -> Result<Paste, String> {
    match platform_images() {
        Ok(images) => Ok(Paste::Images(images)),
        Err(image_error) => match platform_text() {
            Ok(text) if !text.is_empty() => Ok(Paste::Text(text)),
            _ => Err(image_error),
        },
    }
}

/// JXA reaches AppKit's pasteboard directly. It keeps ulo dependency-free but
/// avoids AppleScript's expensive clipboard coercion (roughly 40–60 ms
/// rather than 700 ms for the same screenshot in a local benchmark).
#[cfg(target_os = "macos")]
const SCRIPT: &str = r#"
ObjC.import("AppKit");
function run(argv) {
    const pasteboard = $.NSPasteboard.generalPasteboard;
    const zero = String.fromCharCode(0);
    const paths = [];
    const items = pasteboard.pasteboardItems;
    for (let index = 0; index < items.count; index++) {
        const value = items.objectAtIndex(index).stringForType($("public.file-url"));
        if (value.js) {
            const url = $.NSURL.URLWithString(value);
            if (url.path.js) paths.push(ObjC.unwrap(url.path));
        }
    }
    const types = ObjC.deepUnwrap(pasteboard.types);
    let data = null;
    if (types.includes("public.png")) data = pasteboard.dataForType($("public.png"));
    else if (types.includes("public.tiff")) data = pasteboard.dataForType($("public.tiff"));
    // Export a bitmap even when file URLs exist. Rust prefers valid image
    // files, but can fall back to this representation when a URL is stale or
    // points at something unsupported.
    if (data && !data.writeToFileAtomically($(argv[0]), true)) {
        throw new Error("image write failed");
    }
    if (paths.length > 0) return "files" + zero + paths.join(zero) + zero;
    if (data) return "image";
    return "none";
}
"#;

/// A copied image on macOS: the pasteboard's image files, else its bitmap
/// exported to a private temp file (converted to PNG when it isn't
/// readable as is).
#[cfg(target_os = "macos")]
fn platform_images() -> Result<Vec<ImageInput>, String> {
    use std::os::unix::fs::OpenOptionsExt as _;

    // The system temp dir, not the ulo home: the export is transient and is
    // removed below, and osascript's write itself cannot be size-capped —
    // the bound is enforced when the file is read back.
    let dir = std::env::temp_dir();
    let id = uuid::Uuid::now_v7();
    let raw = dir.join(format!(".ulo-clipboard-{id}.image"));
    let png = dir.join(format!(".ulo-clipboard-{id}.png"));
    let (Some(raw), Some(png)) = (raw.to_str(), png.to_str()) else {
        return Err("clipboard: temp path is not valid unicode".into());
    };
    let raw_path = std::path::PathBuf::from(raw);
    let png_path = std::path::PathBuf::from(png);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&raw_path)
        .map_err(|error| format!("clipboard: {error}"))?;

    let result = read_pasteboard(raw, png);
    let _ = std::fs::remove_file(&raw_path);
    let _ = std::fs::remove_file(&png_path);
    result
}

/// Run the JXA export into `raw`, then prefer the listed image files, else
/// the exported bitmap, converting it into `png` when needed.
#[cfg(target_os = "macos")]
fn read_pasteboard(raw: &str, png: &str) -> Result<Vec<ImageInput>, String> {
    let raw_path = std::path::Path::new(raw);
    let output = run(OSASCRIPT, &["-l", "JavaScript", "-e", SCRIPT, raw]).map_err(unusable)?;
    if !output.success {
        return Err("clipboard could not be read".into());
    }
    let mut file_error = None;
    if let Some(encoded) = output.stdout.strip_prefix(b"files\0") {
        // osascript appends a newline after its result. The JXA result's
        // final NUL marks the exact payload, preserving newlines in names.
        let end = encoded
            .iter()
            .rposition(|byte| *byte == 0)
            .unwrap_or(encoded.len());
        let paths = encoded[..end]
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| String::from_utf8(path.to_vec()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "clipboard file path is not valid UTF-8".to_string())?;
        match ImageInput::from_paths(&paths) {
            Ok(images) => return Ok(images),
            Err(error) => file_error = Some(error),
        }
    }
    if raw_path.metadata().is_ok_and(|metadata| metadata.len() > 0) {
        return match ImageInput::from_path(raw_path) {
            Ok(image) => Ok(vec![image]),
            Err(_) => {
                let converted =
                    run(SIPS, &["-s", "format", "png", raw, "--out", png]).map_err(unusable)?;
                if !converted.success {
                    return Err("clipboard image could not be converted to PNG".into());
                }
                ImageInput::from_path(std::path::Path::new(png)).map(|image| vec![image])
            }
        };
    }
    Err(file_error.unwrap_or_else(|| "clipboard does not contain a supported image".into()))
}

#[cfg(target_os = "macos")]
fn platform_text() -> Result<String, String> {
    let output = run("/usr/bin/pbpaste", &[]).map_err(unusable)?;
    if !output.success {
        return Err("clipboard text could not be read".into());
    }
    String::from_utf8(output.stdout).map_err(|_| "clipboard text is not valid UTF-8".into())
}

#[cfg(target_os = "macos")]
fn unusable(error: RunError) -> String {
    match error {
        RunError::Missing => "clipboard helper is not available".into(),
        RunError::Failed(message) => message,
    }
}

#[cfg(target_os = "linux")]
fn platform_images() -> Result<Vec<ImageInput>, String> {
    for (program, args) in [
        ("wl-paste", vec!["--no-newline", "--type", "text/uri-list"]),
        (
            "xclip",
            vec!["-selection", "clipboard", "-t", "text/uri-list", "-o"],
        ),
    ] {
        match command_output(program, &args) {
            Ok(Some(bytes)) => {
                if let Ok(text) = String::from_utf8(bytes) {
                    let paths = paths_from_uri_list(&text);
                    if !paths.is_empty() {
                        if let Ok(images) = ImageInput::from_paths(&paths) {
                            return Ok(images);
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(error) => return Err(error),
        }
    }
    for mime in ["image/png", "image/jpeg", "image/webp", "image/gif"] {
        for program in ["wl-paste", "xclip"] {
            let args: Vec<&str> = if program == "wl-paste" {
                vec!["--no-newline", "--type", mime]
            } else {
                vec!["-selection", "clipboard", "-t", mime, "-o"]
            };
            match command_output(program, &args) {
                Ok(Some(bytes)) => {
                    if let Ok(image) = ImageInput::from_bytes(bytes) {
                        return Ok(vec![image]);
                    }
                }
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
    }
    Err("clipboard does not contain a supported image".into())
}

/// `Ok(None)`: the helper is absent or reported nothing — try the next one.
/// `Err`: the read failed for a reason worth reporting (timeout, over-cap).
#[cfg(target_os = "linux")]
fn command_output(program: &str, args: &[&str]) -> Result<Option<Vec<u8>>, String> {
    match run(program, args) {
        Ok(output) if output.success && !output.stdout.is_empty() => Ok(Some(output.stdout)),
        Ok(_) => Ok(None),
        Err(RunError::Missing) => Ok(None),
        Err(RunError::Failed(message)) => Err(message),
    }
}

#[cfg(target_os = "linux")]
fn paths_from_uri_list(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let rest = line.strip_prefix("file://")?;
            // `file://localhost/…` means the local host; an empty authority
            // is already the leading slash. Remote authorities stay put.
            let path = match rest.strip_prefix("localhost") {
                Some(local) if local.starts_with('/') => local,
                _ => rest,
            };
            (!path.is_empty()).then_some(path)
        })
        .filter_map(percent_decode)
        .collect()
}

#[cfg(target_os = "linux")]
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let encoded = bytes.get(index + 1..index + 3)?;
            let text = std::str::from_utf8(encoded).ok()?;
            out.push(u8::from_str_radix(text, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(target_os = "linux")]
fn platform_text() -> Result<String, String> {
    for (program, args) in [
        ("wl-paste", vec!["--no-newline"]),
        ("xclip", vec!["-selection", "clipboard", "-o"]),
    ] {
        match command_output(program, &args)? {
            Some(bytes) => {
                return String::from_utf8(bytes)
                    .map_err(|_| "clipboard text is not valid UTF-8".into())
            }
            None => continue,
        }
    }
    Err("clipboard does not contain text".into())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_images() -> Result<Vec<ImageInput>, String> {
    Err("clipboard images are not supported on this platform".into())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_text() -> Result<String, String> {
    Err("clipboard text is not supported on this platform".into())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn uri_lists_decode_multiple_file_paths() {
        assert_eq!(
            super::paths_from_uri_list(
                "# copied files\nfile:///tmp/one%20shot.png\nfile://localhost/tmp/two.jpg\n"
            ),
            ["/tmp/one shot.png", "/tmp/two.jpg"]
        );
    }

    #[test]
    fn a_missing_helper_tries_the_next_one() {
        assert_eq!(
            super::command_output("ulo-definitely-not-installed", &[]),
            Ok(None)
        );
    }
}
