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
  const dir = mkdtempSync(join(tmpdir(), "ulo-slack-restart-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const path = join(dir, "state.json");
  const saved = JSON.parse(readFileSync(new URL("../../../crates/cli/tests/fixtures/channels/slack-state-v1.json", import.meta.url), "utf8"));
  writeFileSync(path, JSON.stringify({ ...saved, unsaved: { session: "dead-process-session" } }));
  const rpc = new FakeRpc();
  const threads = new Threads(path, () => rpc, { cwd: "/repo" }, () => {});
  assert.equal(threads.has("unsaved"), false);
  const thread = await threads.get("C123:123.456", "thread");
  assert.equal(thread.session, "new-process-session");
  assert.equal(rpc.calls.find((c) => c.method === "session.create")?.params.resume, "/saved/conversation.jsonl");
  threads.save("C123:123.456", "/saved/conversation.jsonl");
  assert.deepEqual(JSON.parse(readFileSync(path, "utf8")), {
    "C123:123.456": { path: "/saved/conversation.jsonl" },
  });
});

test("concurrent threads route colliding ask IDs to their own connection", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "ulo-slack-asks-"));
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

test("damaged saved state is reported and preserved", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "ulo-slack-corrupt-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const path = join(dir, "state.json");
  for (const contents of ["{broken", "null", '{"thread":{"path":false}}']) {
    writeFileSync(path, contents);
    assert.throws(() => new Threads(path, () => new FakeRpc(), {}, () => {}));
    assert.equal(readFileSync(path, "utf8"), contents);
  }
});

test("a failed save does not update the in-memory thread map", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "ulo-slack-write-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const threads = new Threads(join(dir, "missing", "state.json"), () => new FakeRpc(), {}, () => {});
  assert.throws(() => threads.save("new", "/saved/new.jsonl"));
  assert.equal(threads.has("new"), false);
});


test("shutdown closes connections still waiting for their first response", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "ulo-slack-opening-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  let reject: (error: Error) => void = () => {};
  const waiting = new Promise<Json>((_, fail) => { reject = fail; });
  const rpc = new FakeRpc();
  rpc.call = () => waiting;
  rpc.close = async () => { reject(new Error("closed")); };
  const threads = new Threads(join(dir, "state.json"), () => rpc, {}, () => {});
  const opening = assert.rejects(threads.get("thread", "name"), /closed/);
  await threads.close();
  await opening;
});
