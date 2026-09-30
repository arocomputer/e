/** Pin restart recovery and ownership when multiple threads ask at once. */
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { Threads, type Connection } from "./threads.ts";
import type { Json } from "./rpc.ts";

/** Record requests and emulate a new process-local session without a provider. */
class FakeRpc implements Connection {
  exit: () => void = () => {};
  exited = new Promise<void>((resolve) => { this.exit = resolve; });
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

test("damaged saved state is reported and preserved", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-corrupt-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const path = join(dir, "state.json");
  for (const contents of ["{broken", "null", '{"thread":{"path":false}}']) {
    writeFileSync(path, contents);
    assert.throws(() => new Threads(path, () => new FakeRpc(), {}, () => {}));
    assert.equal(readFileSync(path, "utf8"), contents);
  }
});

test("a failed save does not update the in-memory thread map", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-write-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const threads = new Threads(join(dir, "missing", "state.json"), () => new FakeRpc(), {}, () => {});
  assert.throws(() => threads.save("new", "/saved/new.jsonl"));
  assert.equal(threads.has("new"), false);
});


test("shutdown closes connections still waiting for their first response", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-opening-"));
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

test("an exited connection is evicted and its saved conversation resumes in a new process", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-exit-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const retired: string[] = [];
  const threads = new Threads(join(dir, "state.json"), () => new FakeRpc(), {}, () => {},
    { onRetire: (key) => retired.push(key) });
  const first = await threads.get("thread", "name");
  threads.save("thread", "/saved/thread.jsonl");
  (first.rpc as FakeRpc).exit();
  await first.rpc.exited;
  const next = await threads.get("thread", "name");
  assert.notEqual(first.rpc, next.rpc);
  assert.deepEqual(retired, ["thread"]);
  assert.equal((next.rpc as FakeRpc).calls.find((c) => c.method === "session.create")?.params.resume, "/saved/thread.jsonl");
  await threads.close();
});

test("idle connections close while a running turn keeps its process alive", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-idle-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  let closes = 0;
  const threads = new Threads(join(dir, "state.json"), () => {
    const rpc = new FakeRpc();
    rpc.close = async () => { closes++; rpc.exit(); };
    return rpc;
  }, {}, () => {}, { idleMs: 20 });
  await threads.get("idle", "idle");
  await threads.use("busy", "busy", async () => {
    await delay(60);
    assert.equal(threads.has("idle"), false);
    assert.equal(threads.has("busy"), true);
    assert.equal(closes, 1);
  });
  await delay(60);
  assert.equal(threads.has("busy"), false);
  assert.equal(closes, 2);
  await threads.close();
});

test("the process limit includes opening connections and capacity returns after exit", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-limit-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  let ready: (value: Json) => void = () => {};
  const rpc = new FakeRpc();
  const waiting = new Promise<Json>((resolve) => { ready = resolve; });
  const call = rpc.call.bind(rpc);
  rpc.call = (method, params) => method === "hello" ? waiting : call(method, params);
  let starts = 0;
  const threads = new Threads(join(dir, "state.json"), () => ++starts === 1 ? rpc : new FakeRpc(),
    {}, () => {}, { maxThreads: 1 });
  const opening = threads.get("first", "first");
  await assert.rejects(threads.get("second", "second"), /limit reached/);
  assert.equal(starts, 1);
  ready({});
  await opening;
  rpc.exit();
  await rpc.exited;
  await threads.get("second", "second");
  assert.equal(starts, 2);
  await threads.close();
  await assert.rejects(threads.get("third", "third"), /shutting down/);
});

test("a second turn cannot replace the running turn's listeners", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-busy-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const threads = new Threads(join(dir, "state.json"), () => new FakeRpc(), {}, () => {});
  await threads.use("thread", "name", async () => {
    await assert.rejects(threads.use("thread", "name", async () => {}), /already has a running turn/);
  });
  await threads.close();
});

test("idle limits cannot overflow Node's timer and retire sessions immediately", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "e-slack-config-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  assert.throws(() => new Threads(join(dir, "state.json"), () => new FakeRpc(), {}, () => {},
    { idleMs: 2_147_483_648 }), /timer limit/);
});
