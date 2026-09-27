//! e's terminal in a web page.
//!
//! [`Session::start`] runs the real TUI and agent on the page's event loop.
//! Output goes to a terminal emulator through a callback, keys come back
//! through [`Session::input`], files live in an in-memory workspace, and
//! `bash` commands go to a shell the page supplies (in the dev page, a
//! simulated one working on the same files through [`Session::read`] and
//! its siblings). Nothing reaches a disk or starts a process.
//!
//! One session per page: the terminal it draws on is page-wide.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use e_core::agent::AgentOptions;
use e_core::providers::catalog::{self, Api};
use e_core::tools::workspace::{Kind, Memory};
use e_core::tools::{Shell, ShellOutput, Workspace};
use js_sys::{Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

/// A running session and its workspace.
#[wasm_bindgen]
pub struct Session {
    workspace: Arc<Memory>,
    exited: Promise,
}

#[wasm_bindgen]
impl Session {
    /// Start a session. `options` is JSON:
    ///
    /// - `cols`, `rows`: the terminal's size; `light`: whether its
    ///   background is light.
    /// - `provider`, `model`, `base_url`, `api` (`completions`,
    ///   `responses`, `anthropic`, or `google`), `api_key`: the model and
    ///   the endpoint that serves it.
    /// - `cwd`: the project directory, default `/project`; `files`: an
    ///   object of path to text, seeded before the first frame.
    ///
    /// `output` receives every byte e writes, as a `Uint8Array`. `shell`,
    /// when given, is called as `shell(command, cwd)` and resolves with
    /// `{stdout, stderr, exitCode}`; without it, `bash` fails.
    pub fn start(
        options: &str,
        output: Function,
        shell: Option<Function>,
    ) -> Result<Session, JsError> {
        report_panics();
        let options: serde_json::Value = serde_json::from_str(options)?;
        let text = |key: &str| options[key].as_str().map(str::to_string);
        let number = |key: &str, default: u16| {
            options[key]
                .as_u64()
                .map_or(default, |value| value.min(u64::from(u16::MAX)) as u16)
        };

        let cwd = PathBuf::from(text("cwd").unwrap_or_else(|| "/project".into()));
        let workspace = Arc::new(Memory::new());
        workspace.create_dir_all(&cwd)?;
        if let Some(files) = options["files"].as_object() {
            for (path, contents) in files {
                let path = cwd.join(path);
                if let Some(parent) = path.parent() {
                    workspace.create_dir_all(parent)?;
                }
                workspace.write(&path, contents.as_str().unwrap_or_default().as_bytes())?;
            }
        }

        let api = text("api").unwrap_or_else(|| "completions".into());
        let api = Api::parse(&api).ok_or_else(|| JsError::new(&format!("unknown api `{api}`")))?;
        let (Some(provider), Some(id), Some(base_url)) =
            (text("provider"), text("model"), text("base_url"))
        else {
            return Err(JsError::new("options need provider, model, and base_url"));
        };
        let mut model = catalog::custom(&provider, &id, &base_url, api);
        model.api_key = text("api_key").map(e_core::auth::ApiKey::new);

        e_tui::term::web::install(
            move |bytes| {
                let _ = output.call1(&JsValue::NULL, &Uint8Array::from(bytes));
            },
            number("cols", 80),
            number("rows", 24),
            options["light"].as_bool().unwrap_or(false),
        );
        let agent = AgentOptions {
            cwd: Some(cwd),
            // Nothing is read from or written to a home: settings, sessions,
            // and credentials stay at their defaults for the page's life.
            home: Some(PathBuf::from("/home")),
            save_session: false,
            workspace: Some(workspace.clone()),
            shell: shell.map(|run| Arc::new(PageShell(run)) as Arc<dyn Shell>),
            ..AgentOptions::default()
        };
        let (jobs_tx, jobs_rx) = tokio::sync::mpsc::channel(16);
        let requests = tokio::sync::mpsc::channel(16);
        let run = e_tui::app::run(
            e_tui::app::RunOptions {
                update: None,
                initial: String::new(),
                continue_session: false,
                resume_session: false,
                model,
                agent,
                images: Vec::new(),
            },
            e_core::extensions::ExtensionHost::empty(),
            jobs_tx,
            jobs_rx,
            requests,
        );
        let exited = wasm_bindgen_futures::future_to_promise(async move {
            run.await
                .map(|()| JsValue::UNDEFINED)
                .map_err(|error| JsValue::from_str(&error.to_string()))
        });
        Ok(Session { workspace, exited })
    }

    /// Resolves when the session ends: `/quit`, ctrl+c twice, or [`Session::close`].
    pub fn exited(&self) -> Promise {
        self.exited.clone()
    }

    /// Bytes the terminal emulator reports: keys, pastes, mouse reports.
    pub fn input(&self, bytes: &[u8]) {
        e_tui::term::web::feed(bytes);
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        e_tui::term::web::resize(cols, rows);
    }

    /// End the session; [`Session::exited`] resolves once it has.
    pub fn close(&self) {
        e_tui::term::web::close();
    }

    /// A file's bytes, or `undefined` when it is missing or a directory.
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.workspace.read(Path::new(path)).ok()
    }

    pub fn write(&self, path: &str, bytes: &[u8]) -> Result<(), JsError> {
        Ok(self.workspace.write(Path::new(path), bytes)?)
    }

    /// `"file"` or `"dir"`, or `undefined` when nothing is there.
    pub fn kind(&self, path: &str) -> Option<String> {
        match self.workspace.metadata(Path::new(path)).ok()?.kind {
            Kind::Dir => Some("dir".into()),
            _ => Some("file".into()),
        }
    }

    /// The names inside a directory.
    pub fn list(&self, path: &str) -> Result<Vec<String>, JsError> {
        Ok(self
            .workspace
            .read_dir(Path::new(path))?
            .iter()
            .filter_map(|entry| entry.file_name()?.to_str().map(str::to_string))
            .collect())
    }

    pub fn mkdir(&self, path: &str, recursive: bool) -> Result<(), JsError> {
        let path = Path::new(path);
        if recursive {
            self.workspace.create_dir_all(path)?;
        } else {
            self.workspace.create_dir(path)?;
        }
        Ok(())
    }

    /// Remove a file, or a directory: only an empty one unless `recursive`.
    pub fn remove(&self, path: &str, recursive: bool) -> Result<(), JsError> {
        let path = Path::new(path);
        let is_dir = self.workspace.metadata(path)?.is_dir();
        match (is_dir, recursive) {
            (true, true) => self.workspace.remove_all(path)?,
            (true, false) => self.workspace.remove_dir(path)?,
            (false, _) => self.workspace.remove_file(path)?,
        }
        Ok(())
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), JsError> {
        Ok(self.workspace.rename(Path::new(from), Path::new(to))?)
    }

    /// Every path in the workspace.
    pub fn paths(&self) -> Vec<String> {
        self.workspace
            .paths()
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    }
}

/// The page's shell: `run(command, cwd)` resolves with
/// `{stdout, stderr, exitCode}`.
#[derive(Debug)]
struct PageShell(Function);

impl Shell for PageShell {
    fn run(
        &self,
        command: &str,
        cwd: &Path,
    ) -> e_core::rt::BoxFuture<'static, io::Result<ShellOutput>> {
        let called = self.0.call2(
            &JsValue::NULL,
            &JsValue::from_str(command),
            &JsValue::from_str(&cwd.to_string_lossy()),
        );
        Box::pin(async move {
            let result = JsFuture::from(Promise::resolve(&called.map_err(js_error)?))
                .await
                .map_err(js_error)?;
            let text = |key: &str| {
                Reflect::get(&result, &JsValue::from_str(key))
                    .ok()
                    .and_then(|value| value.as_string())
                    .unwrap_or_default()
                    .into_bytes()
            };
            let exit_code = Reflect::get(&result, &JsValue::from_str("exitCode"))
                .ok()
                .and_then(|value| value.as_f64())
                .unwrap_or(1.0) as i32;
            Ok(ShellOutput {
                stdout: text("stdout"),
                stderr: text("stderr"),
                exit_code,
            })
        })
    }
}

fn js_error(error: JsValue) -> io::Error {
    let message = error
        .dyn_ref::<js_sys::Error>()
        .map(|error| String::from(error.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "the page's shell failed".into());
    io::Error::other(message)
}

/// A panic aborts the module. Say why, on the console and in the terminal,
/// before it does.
fn report_panics() {
    std::panic::set_hook(Box::new(|info| {
        let message = format!("e stopped: {info}");
        console_error(&message);
        let _ = e_tui::term::write(format!("\r\n\x1b[31m{message}\x1b[0m\r\n").as_bytes());
    }));
}
