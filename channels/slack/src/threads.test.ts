/** Pin restart recovery and ownership when multiple threads ask at once. */
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Threads, type Connection } from "./threads.ts";
import type { Json } from "./rpc.ts";

/** Record requests and emulate a new process-local session without a provider. */
class FakeRpc implements Connection {
  calls: { method: string; params: Json }[] = [];
  listeners = new Map<string, (event: Json) => void>();
  onAsk: (ask: Json) => void = () => {};
  async call(method: string, params: Json = {}): Promise<Json> {
    this.calls.push({ method, params });
    return method === "session.create" ? { session: "new-process-session" } : {};
  }
  async close() {}
}

test("restart resumes the saved path and never reuses a persisted session ID", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-restart-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const path = join(dir, "state.json");
  writeFileSync(path, readFileSync(new URL("../../../tests/fixtures/channels/slack-state-v1.json", import.meta.url)));
  const rpc = new FakeRpc();
  const threads = new Threads(path, () => rpc, { cwd: "/repo" }, () => {});
  const thread = await threads.get("C123:123.456", "thread");
  assert.equal(thread.session, "new-process-session");
  assert.equal(rpc.calls.find((c) => c.method === "session.create")?.params.resume, "/saved/conversation.jsonl");
  threads.save("C123:123.456", "/saved/conversation.jsonl");
  assert.deepEqual(JSON.parse(readFileSync(path, "utf8")), {
    "C123:123.456": { path: "/saved/conversation.jsonl" },
  });
});

test("concurrent threads route colliding ask IDs to their own connection", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-asks-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const received: { key: string; rpc: Connection; ask: Json }[] = [];
  const threads = new Threads(join(dir, "state.json"), () => new FakeRpc(), {},
    (key, rpc, ask) => received.push({ key, rpc, ask }));
  const [a, b] = await Promise.all([threads.get("A", "a"), threads.get("B", "b")]);
  assert.notEqual(a.rpc, b.rpc);
  // B was opened most recently, but A's question still belongs to A.
  a.rpc.onAsk({ ask: 1, method: "ui.confirm" });
  b.rpc.onAsk({ ask: 1, method: "ui.confirm" });
  assert.deepEqual(received.map((q) => q.key), ["A", "B"]);
  for (const q of received) await q.rpc.call("ask.reply", { ask: q.ask.ask, result: { text: q.key } });
  assert.deepEqual((a.rpc as FakeRpc).calls.at(-1)?.params, { ask: 1, result: { text: "A" } });
  assert.deepEqual((b.rpc as FakeRpc).calls.at(-1)?.params, { ask: 1, result: { text: "B" } });
});
