import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, writeFile, rm, access } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/** Render the terminal source at 30 fps, then encode a seekable H.264 video. */
const root = fileURLToPath(new URL("../../", import.meta.url));
const origin = process.argv[3] || "http://localhost:3107";
const chrome = process.argv[2];
if (!chrome)
  throw new Error(
    "Usage: node scripts/video/render.mjs /path/to/chrome [dev-origin]",
  );
const route = join(root, "src/pages/demo-export.astro");
try {
  await access(route);
  throw new Error("demo-export already exists; refusing to replace it");
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
const scratch = await mkdtemp(join(tmpdir(), "ulo-video-"));
let browser;
let socket;
try {
  await writeFile(
    route,
    '---\nimport VideoFrame from "../components/demo/export/frame";\n---\n<VideoFrame client:load />\n',
  );
  browser = spawn(
    chrome,
    [
      "--headless=new",
      "--remote-debugging-port=0",
      "--no-first-run",
      "--no-default-browser-check",
      `--user-data-dir=${join(scratch, "profile")}`,
      "about:blank",
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );
  const endpoint = await new Promise((resolve, reject) => {
    browser.once("error", reject);
    let log = "";
    browser.stderr.on("data", (chunk) => {
      log += chunk;
      const match = log.match(/DevTools listening on (ws:\/\/[^\s]+)/);
      if (match) resolve(match[1]);
    });
    browser.once("exit", () =>
      reject(new Error("Chrome exited before opening its debugger")),
    );
  });
  const debugOrigin = endpoint.replace(/^ws:/, "http:").split("/devtools/")[0];
  const tabs = await (await fetch(`${debugOrigin}/json`)).json();
  socket = new WebSocket(
    tabs.find((tab) => tab.type === "page").webSocketDebuggerUrl,
  );
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = reject;
  });
  let id = 0;
  const pending = new Map();
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    const call = pending.get(message.id);
    if (!call) return;
    pending.delete(message.id);
    if (message.error) call.reject(new Error(message.error.message));
    else call.resolve(message.result);
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const number = ++id;
      pending.set(number, { resolve, reject });
      socket.send(JSON.stringify({ id: number, method, params }));
    });
  const evaluate = async (expression) => {
    const result = await send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails)
      throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  await send("Emulation.setDeviceMetricsOverride", {
    width: 1080,
    height: 676,
    deviceScaleFactor: 2,
    mobile: false,
  });
  await send("Page.navigate", { url: `${origin}/demo-export` });
  let duration;
  for (let attempt = 0; attempt < 150; attempt++) {
    duration = await evaluate("window.demoDuration");
    if (duration) break;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  if (!duration)
    throw new Error("The export page did not load. Check the dev server.");
  await evaluate(
    `document.fonts.ready.then(() => { window.exportAnimations = new Map(); })`,
  );
  const fps = 30;
  const count = Math.ceil((duration / 1000) * fps);
  for (let frame = 0; frame < count; frame++) {
    const time = (frame * 1000) / fps;
    await evaluate(`(() => {
      window.renderDemoFrame(${time});
      const screen = document.querySelector('.ulo-demo-screen pre');
      getComputedStyle(screen).transform;
      for (const animation of screen.getAnimations()) {
        if (!window.exportAnimations.has(animation)) window.exportAnimations.set(animation, ${time});
        animation.pause();
        animation.currentTime = ${time} - window.exportAnimations.get(animation);
      }
    })()`);
    const shot = await send("Page.captureScreenshot", {
      format: "png",
      clip: { x: 0, y: 0, width: 1080, height: 676, scale: 1 },
    });
    await writeFile(
      join(scratch, `${String(frame).padStart(5, "0")}.png`),
      Buffer.from(shot.data, "base64"),
    );
    if (frame % 150 === 0) console.log(`Rendered ${frame}/${count}`);
  }
  const output = join(root, "public/demo.mp4");
  execFileSync("ffmpeg", [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-framerate",
    String(fps),
    "-i",
    join(scratch, "%05d.png"),
    "-c:v",
    "libx264",
    "-preset",
    "slow",
    "-crf",
    "18",
    "-pix_fmt",
    "yuv420p",
    "-movflags",
    "+faststart",
    output,
  ]);
  execFileSync("ffmpeg", [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-i",
    output,
    "-frames:v",
    "1",
    join(root, "public/demo-poster.jpg"),
  ]);
  console.log(`Wrote ${output}, ${(count / fps).toFixed(2)} seconds`);
} finally {
  socket?.close();
  browser?.kill();
  await rm(route, { recursive: true, force: true });
  await rm(scratch, {
    recursive: true,
    force: true,
    maxRetries: 10,
    retryDelay: 200,
  });
}
