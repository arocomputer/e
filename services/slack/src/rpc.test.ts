/** Shutdown must reap an unresponsive child and settle its pending calls. */
import assert from "node:assert/strict";
import { test } from "node:test";
import { Rpc } from "./rpc.ts";

test("agent children exclude Slack credentials while preserving provider and runtime settings", async (t) => {
  const source = { SLACK_BOT_TOKEN: "bot", SLACK_APP_TOKEN: "app", SLACK_SIGNING_SECRET: "signing",
    SLACK_FUTURE_SECRET: "future", ANTHROPIC_API_KEY: "provider", E_HOME: "/state" };
  const previous = Object.fromEntries(Object.keys(source).map((key) => [key, process.env[key]]));
  t.after(() => {
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  });
  Object.assign(process.env, source);
  const rpc = new Rpc(process.execPath, ["-e", `
    process.stdin.on("data", (data) => {
      const request = JSON.parse(data.toString());
      const keys = ${JSON.stringify(Object.keys(source))};
      const result = Object.fromEntries(keys.map((key) => [key, process.env[key] ?? null]));
      process.stdout.write(JSON.stringify({ id: request.id, result }) + "\\n");
    });
  `]);
  t.after(() => rpc.close(50));
  assert.deepEqual(await rpc.call("hello"), {
    SLACK_BOT_TOKEN: null, SLACK_APP_TOKEN: null, SLACK_SIGNING_SECRET: null,
    SLACK_FUTURE_SECRET: null, ANTHROPIC_API_KEY: "provider", E_HOME: "/state",
  });
  assert.equal(process.env.SLACK_BOT_TOKEN, "bot");
});

test("shutdown kills an unresponsive process and rejects pending requests", { timeout: 5000 }, async () => {
  const rpc = new Rpc(process.execPath, ["-e", 'process.on("SIGTERM", () => {}); process.stdin.resume(); setInterval(() => {}, 1000)']);
  const pending = assert.rejects(rpc.call("hello"), /exited/);
  await rpc.close(100);
  await pending;
});


test("a failed spawn can be closed without waiting for an exit event", { timeout: 5000 }, async () => {
  const rpc = new Rpc("/does-not-exist/e", []);
  await assert.rejects(rpc.call("hello"), /ENOENT|EPIPE/);
  await rpc.close(50);
});
