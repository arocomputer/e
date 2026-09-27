# e in the browser (archived)

e's own terminal interface and agent, compiled to WebAssembly and drawing
into a terminal emulator on a web page. Files live in an in-memory
workspace, `bash` runs in a shell the page supplies, and the model is
reached over the page's `fetch`. Nothing touches a disk or starts a process.

This is an archived experiment, not a product: nothing links to it, no
release ships it, and `./x check` does not build it. It has its own Cargo
workspace for that reason. What it proved is kept in the core and the SDK
(the workspace, shell, and runtime seams); this crate is the browser glue.

## Run it

```sh
cargo install wasm-bindgen-cli --version 0.2.128 --locked   # once
crates/web/build.sh
cd crates/web/dev && npm install && npm start
```

Open <http://localhost:8765>. The model is a scripted mock in `serve.mjs`
that fixes a bug in the seeded project with one `bash` call and one `edit`.
For a real model, start the server with an OpenAI-compatible endpoint and
open the page with `?upstream`; the server adds the key, so the page never
holds it:

```sh
E_WEB_UPSTREAM=https://openrouter.ai/api/v1 E_WEB_KEY=… E_WEB_MODEL=… npm start
```

## How it fits together

- `src/lib.rs`: `Session.start(options, output, shell)` builds the model
  (`catalog::custom` with the key), an in-memory workspace, and the page's
  shell, then runs `e_tui::app::run`. `input`, `resize`, and `close` feed the
  terminal; `read`, `write`, `list`, and friends expose the workspace to the
  page's shell.
- `e_tui::term::web` is the terminal underneath: the page's output sink, and
  input bytes decoded by `e_tui::term::vt`.
- `e_core::rt` runs tasks and timers on the page's event loop, so the build
  needs nothing beyond WebAssembly itself (no JSPI).
- `dev/main.js`: xterm.js, and just-bash with a filesystem adapter over the
  session's workspace, so the shell and e's tools see the same files.

The wasm is about 3.2 MB, 1.1 MB compressed.

## Known gaps

- Project resources (`AGENTS.md`, skills, prompts) are read from disk, so a
  browser session has none.
- Settings, history, and sessions are not kept between page loads.
- Extensions, background commands, the external editor, and clipboard images
  need the native build.
