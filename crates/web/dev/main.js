// The dev page: e's terminal (crates/web) drawing into xterm.js, with a
// simulated shell (just-bash) working on the session's in-memory files.
// The model is the mock in serve.mjs unless the page is opened with
// `?upstream`, which goes through serve.mjs's proxy to a real provider.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Bash } from "just-bash/browser";
import init, { Session } from "./pkg/e_web.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** A Node-style filesystem error, the shape just-bash reports. */
function fsError(code, text, operation, path) {
  const error = new Error(`${code}: ${text}, ${operation} '${path}'`);
  error.code = code;
  return error;
}

/** Normalize `path` against `base` the way a shell would. */
function resolvePath(base, path) {
  const parts = [];
  for (const part of (path.startsWith("/") ? path : `${base}/${path}`).split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  return `/${parts.join("/")}`;
}

/**
 * just-bash's filesystem interface over the session's workspace, so the
 * shell and e's own tools see the same files.
 */
class WorkspaceFs {
  constructor(session) {
    this.session = session;
  }

  bytes(path, operation) {
    const at = resolvePath("/", path);
    const kind = this.session.kind(at);
    if (kind === undefined) throw fsError("ENOENT", "no such file or directory", operation, path);
    if (kind === "dir") throw fsError("EISDIR", "illegal operation on a directory", operation, path);
    return this.session.read(at);
  }

  async readFile(path) {
    return decoder.decode(this.bytes(path, "open"));
  }

  async readFileBuffer(path) {
    return this.bytes(path, "open");
  }

  async readFileBytes(path) {
    const bytes = this.bytes(path, "open");
    let text = "";
    for (let at = 0; at < bytes.length; at += 32768) {
      text += String.fromCharCode(...bytes.subarray(at, at + 32768));
    }
    return text;
  }

  async writeFile(path, content) {
    const at = resolvePath("/", path);
    const parent = at.slice(0, at.lastIndexOf("/")) || "/";
    if (this.session.kind(parent) !== "dir") {
      throw fsError("ENOENT", "no such file or directory", "open", path);
    }
    this.session.write(at, typeof content === "string" ? encoder.encode(content) : content);
  }

  async appendFile(path, content) {
    const at = resolvePath("/", path);
    const before = this.session.kind(at) === "file" ? this.session.read(at) : new Uint8Array();
    const added = typeof content === "string" ? encoder.encode(content) : content;
    const joined = new Uint8Array(before.length + added.length);
    joined.set(before);
    joined.set(added, before.length);
    await this.writeFile(path, joined);
  }

  async exists(path) {
    return this.session.kind(resolvePath("/", path)) !== undefined;
  }

  async stat(path) {
    const at = resolvePath("/", path);
    const kind = this.session.kind(at);
    if (kind === undefined) throw fsError("ENOENT", "no such file or directory", "stat", path);
    return {
      isFile: kind === "file",
      isDirectory: kind === "dir",
      isSymbolicLink: false,
      mode: kind === "dir" ? 0o755 : 0o644,
      size: kind === "file" ? this.session.read(at).length : 0,
      mtime: new Date(),
    };
  }

  async lstat(path) {
    return this.stat(path);
  }

  async mkdir(path, options) {
    const at = resolvePath("/", path);
    if (this.session.kind(at) !== undefined) {
      if (options?.recursive) return;
      throw fsError("EEXIST", "file already exists", "mkdir", path);
    }
    try {
      this.session.mkdir(at, Boolean(options?.recursive));
    } catch {
      throw fsError("ENOENT", "no such file or directory", "mkdir", path);
    }
  }

  async readdir(path) {
    const at = resolvePath("/", path);
    const kind = this.session.kind(at);
    if (kind === undefined) throw fsError("ENOENT", "no such file or directory", "scandir", path);
    if (kind !== "dir") throw fsError("ENOTDIR", "not a directory", "scandir", path);
    return this.session.list(at).sort();
  }

  async readdirWithFileTypes(path) {
    const at = resolvePath("/", path);
    return (await this.readdir(path)).map((name) => {
      const kind = this.session.kind(`${at === "/" ? "" : at}/${name}`);
      return { name, isFile: kind === "file", isDirectory: kind === "dir", isSymbolicLink: false };
    });
  }

  async rm(path, options) {
    const at = resolvePath("/", path);
    if (this.session.kind(at) === undefined) {
      if (options?.force) return;
      throw fsError("ENOENT", "no such file or directory", "rm", path);
    }
    try {
      this.session.remove(at, Boolean(options?.recursive));
    } catch {
      throw fsError("ENOTEMPTY", "directory not empty", "rm", path);
    }
  }

  async cp(source, destination, options) {
    const from = resolvePath("/", source);
    const to = resolvePath("/", destination);
    if (this.session.kind(from) === "dir") {
      if (!options?.recursive) throw fsError("EISDIR", "illegal operation on a directory", "cp", source);
      await this.mkdir(to, { recursive: true });
      for (const name of this.session.list(from)) {
        await this.cp(`${from}/${name}`, `${to}/${name}`, options);
      }
      return;
    }
    await this.writeFile(to, this.bytes(from, "cp"));
  }

  async mv(source, destination) {
    const from = resolvePath("/", source);
    if (this.session.kind(from) === undefined) {
      throw fsError("ENOENT", "no such file or directory", "mv", source);
    }
    this.session.rename(from, resolvePath("/", destination));
  }

  resolvePath(base, path) {
    return resolvePath(base, path);
  }

  getAllPaths() {
    return this.session.paths();
  }

  async realpath(path) {
    const at = resolvePath("/", path);
    if (this.session.kind(at) === undefined) {
      throw fsError("ENOENT", "no such file or directory", "realpath", path);
    }
    return at;
  }

  async chmod() {}

  async utimes() {}

  async symlink(target, path) {
    throw fsError("ENOTSUP", "links are not supported", "symlink", path);
  }

  async link(existing, path) {
    throw fsError("ENOTSUP", "links are not supported", "link", path);
  }

  async readlink(path) {
    throw fsError("EINVAL", "invalid argument", "readlink", path);
  }
}

/** A few files so the first prompt has something to work on. */
const PROJECT = {
  "README.md": "# tally\n\nCounts words in text files.\n\n    node src/tally.js notes.txt\n",
  "src/tally.js":
    'const fs = require("fs");\n\nfunction tally(text) {\n  return text.split(" ").length;\n}\n\nconsole.log(tally(fs.readFileSync(process.argv[2], "utf8")));\n',
  "notes.txt": "one two  three\nfour\n",
};

const dark = matchMedia("(prefers-color-scheme: dark)").matches;
const terminal = new Terminal({
  fontFamily: '"IBM Plex Mono", ui-monospace, Menlo, monospace',
  fontSize: 14,
  cursorBlink: true,
  scrollback: 10000,
  theme: dark
    ? { background: "#10131c", foreground: "#f0f0f0", cursor: "#f0f0f0", selectionBackground: "#3a4466" }
    : { background: "#f8f8f8", foreground: "#10131c", cursor: "#10131c", selectionBackground: "#b9bfd3" },
});
const fit = new FitAddon();
terminal.loadAddon(fit);
terminal.open(document.getElementById("terminal"));
fit.fit();

await init();
const upstream = new URLSearchParams(location.search).has("upstream");
let bash;
const session = Session.start(
  JSON.stringify({
    cols: terminal.cols,
    rows: terminal.rows,
    light: !dark,
    provider: upstream ? "upstream" : "mock",
    model: upstream ? (await (await fetch("/upstream-model")).text()) : "scripted",
    base_url: `${location.origin}/${upstream ? "upstream" : "mock/v1"}`,
    api: "completions",
    // serve.mjs adds the real key upstream; the browser never holds one.
    api_key: "page",
    files: PROJECT,
  }),
  (bytes) => terminal.write(bytes),
  async (command, cwd) => {
    const result = await bash.exec(command, { cwd });
    return { stdout: result.stdout, stderr: result.stderr, exitCode: result.exitCode };
  },
);
bash = new Bash({ fs: new WorkspaceFs(session), cwd: "/project" });

terminal.onData((data) => session.input(encoder.encode(data)));
terminal.onBinary((data) => session.input(Uint8Array.from(data, (c) => c.charCodeAt(0))));
terminal.onResize(({ cols, rows }) => session.resize(cols, rows));
new ResizeObserver(() => fit.fit()).observe(document.getElementById("terminal"));
terminal.attachCustomKeyEventHandler((event) => {
  if (event.type !== "keydown") return true;
  // xterm.js sends a plain carriage return for shift+enter; say which it
  // was, in the kitty protocol e asked for, so it inserts a newline.
  if (event.key === "Enter" && event.shiftKey) {
    session.input(encoder.encode("\x1b[13;2u"));
    return false;
  }
  // Leave paste to the browser: it arrives as a bracketed paste.
  if (event.key === "v" && (event.ctrlKey || event.metaKey)) return false;
  return true;
});
terminal.focus();

session.exited().then(
  () => terminal.write("\r\n[session ended — reload to start again]\r\n"),
  (error) => terminal.write(`\r\n[session failed: ${error}]\r\n`),
);
// For inspecting a running session from the console.
window.e = { terminal, session, bash };
