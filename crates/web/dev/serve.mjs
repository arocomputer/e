// Serve the dev page for crates/web on http://localhost:8765 (PORT to
// change it): bundle main.js, serve the page and the wasm build, and answer
// model requests.
//
// The model is a scripted mock at /mock/v1 unless E_WEB_UPSTREAM (an
// OpenAI-compatible base URL), E_WEB_KEY, and E_WEB_MODEL are set; then
// /upstream proxies to it with the key added here, and `?upstream` on the
// page uses it. The browser never holds the key.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const root = fileURLToPath(new URL(".", import.meta.url));
const port = Number(process.env.PORT ?? 8765);

// just-bash's browser build still names a few Node modules for commands a
// browser cannot run (gzip and friends). They get a module that says so.
const unavailable = 'function unavailable() { throw new Error("not available in the browser"); }';
const nodeStubs = {
  name: "node-stubs",
  setup(build) {
    build.onResolve({ filter: /^node:/ }, (args) => ({ path: args.path, namespace: "stub" }));
    build.onLoad({ filter: /.*/, namespace: "stub" }, (args) => ({
      contents:
        args.path === "node:zlib"
          ? `${unavailable} export const constants = {}; export { unavailable as gzipSync, unavailable as gunzipSync };`
          : "export default {};",
    }));
  },
};

await esbuild.build({
  entryPoints: [join(root, "main.js")],
  outfile: join(root, "bundle.js"),
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  external: ["./pkg/e_web.js"],
  plugins: [nodeStubs],
  logLevel: "warning",
});

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json",
};

/** One Server-Sent Events chunk in the Chat Completions shape. */
const event = (payload) => `data: ${JSON.stringify(payload)}\n\n`;
const text = (content) => event({ choices: [{ delta: { content } }] });
const call = (name, args) =>
  event({
    choices: [
      { delta: { tool_calls: [{ index: 0, id: `call_${name}`, function: { name, arguments: JSON.stringify(args) } }] } },
    ],
  }) + event({ choices: [{ finish_reason: "tool_calls" }] });

/**
 * A model that fixes the word count in the seeded project, one step per
 * request: look with the shell, edit with e's own tool, then report. The
 * step is how many tool results the conversation already holds.
 */
function scripted(body) {
  const results = body.messages.filter((message) => message.role === "tool").length;
  const steps = [
    () => text("Counting the words in notes.txt with the shell first.") + call("bash", { command: "wc -w notes.txt" }),
    () =>
      text("There are four words, but `split(\" \")` counts the empty string between the two spaces. ") +
      call("edit", {
        path: "src/tally.js",
        old_string: 'return text.split(" ").length;',
        new_string: "return text.split(/\\s+/).filter(Boolean).length;",
      }),
    () =>
      ["Fixed ", "`tally` to split on any run of whitespace ", "and drop empty pieces, ", "so it now agrees with `wc -w`."]
        .map(text)
        .join(""),
  ];
  const step = steps[Math.min(results, steps.length - 1)];
  return step() + event({ choices: [], usage: { prompt_tokens: 1200, completion_tokens: 40 } }) + "data: [DONE]\n\n";
}

async function readBody(request) {
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}

async function proxy(request, response) {
  const upstream = process.env.E_WEB_UPSTREAM;
  if (!upstream || !process.env.E_WEB_KEY) {
    response.writeHead(503).end("set E_WEB_UPSTREAM and E_WEB_KEY to use a real model");
    return;
  }
  const target = upstream.replace(/\/$/, "") + request.url.slice("/upstream".length);
  const answer = await fetch(target, {
    method: request.method,
    headers: { "content-type": "application/json", authorization: `Bearer ${process.env.E_WEB_KEY}` },
    body: request.method === "POST" ? await readBody(request) : undefined,
  });
  response.writeHead(answer.status, { "content-type": answer.headers.get("content-type") ?? "text/plain" });
  for await (const chunk of answer.body) response.write(chunk);
  response.end();
}

createServer(async (request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  try {
    if (path === "/mock/v1/chat/completions" && request.method === "POST") {
      response.writeHead(200, { "content-type": "text/event-stream" });
      response.end(scripted(JSON.parse(await readBody(request))));
      return;
    }
    if (path === "/upstream-model") {
      response.end(process.env.E_WEB_MODEL ?? "");
      return;
    }
    if (path.startsWith("/upstream/")) {
      await proxy(request, response);
      return;
    }
    const file = normalize(join(root, path === "/" ? "index.html" : path));
    if (!file.startsWith(root)) {
      response.writeHead(403).end();
      return;
    }
    const contents = await readFile(file);
    response.writeHead(200, {
      "content-type": TYPES[extname(file)] ?? "application/octet-stream",
      "cache-control": "no-store",
    });
    response.end(contents);
  } catch (error) {
    response.writeHead(error.code === "ENOENT" ? 404 : 500).end(String(error.message));
  }
}).listen(port, () => console.log(`e in the browser: http://localhost:${port}`));
