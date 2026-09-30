/** Verify authorization is installed on the actual adapter, including approval handlers. */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import bolt from "@slack/bolt";
import type { AnyMiddlewareArgs, Middleware } from "@slack/bolt";

test("the running adapter gates every registered prompt and question handler", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-adapter-"));
  const values = { E_CWD: dir, E_SLACK_STATE: join(dir, "state.json"),
    E_SLACK_ALLOWED_USERS: "U123", E_SLACK_ALLOWED_CHANNELS: "C123",
    E_SLACK_MAX_THREADS: "16", E_SLACK_IDLE_MS: "900000",
    SLACK_BOT_TOKEN: "test-bot", SLACK_APP_TOKEN: "test-app", SLACK_SIGNING_SECRET: "test-signing" };
  const previous = Object.fromEntries(Object.keys(values).map((key) => [key, process.env[key]]));
  const signals = new Map((["SIGINT", "SIGTERM"] as const).map((signal) => [signal, process.listeners(signal)]));
  const originalApp = bolt.App;
  let app: FakeApp;
  class FakeApp {
    middleware: Middleware<AnyMiddlewareArgs>[] = [];
    handlers: string[] = [];
    constructor() { app = this; }
    use(middleware: Middleware<AnyMiddlewareArgs>) { this.middleware.push(middleware); }
    action(name: string | RegExp) { this.handlers.push(String(name)); }
    event(name: string) { this.handlers.push(name); }
    message() { this.handlers.push("message"); }
    async start() {}
  }
  t.after(() => {
    (bolt as unknown as { App: unknown }).App = originalApp;
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    for (const [signal, before] of signals) {
      for (const listener of process.listeners(signal)) {
        if (!before.includes(listener)) process.removeListener(signal, listener);
      }
    }
    rmSync(dir, { recursive: true, force: true });
  });
  Object.assign(process.env, values);
  (bolt as unknown as { App: unknown }).App = FakeApp;
  await import(new URL("./index.ts", import.meta.url).href);
  assert.deepEqual(app!.handlers, ["ask_yes", "ask_no", "/^ask_pick_/", "app_mention", "message"]);
  for (const handler of app!.handlers) {
    for (const allowed of [false, true]) {
      const user = allowed ? "U123" : "U999";
      const body = handler.startsWith("ask_") || handler.startsWith("/")
        ? { user: { id: user }, channel: { id: "C123" } }
        : { event: { user, channel: "C123", type: handler } };
      let reached = false;
      const dispatch = async (index: number): Promise<void> => {
        const middleware = app!.middleware[index];
        if (!middleware) { reached = true; return; }
        await middleware({ body, ack: async () => {}, next: () => dispatch(index + 1),
        } as Parameters<typeof middleware>[0]);
      };
      await dispatch(0);
      assert.equal(reached, allowed, `${handler}: unauthorized caller reached the handler`);
    }
  }
});
