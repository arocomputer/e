/** Shutdown must reap an unresponsive child and settle its pending calls. */
import assert from "node:assert/strict";
import { test } from "node:test";
import { Rpc } from "./rpc.ts";

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
