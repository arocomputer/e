/** Live RPC processes belong to threads; only saved log paths survive a restart. */
import { readFileSync, writeFileSync, renameSync, unlinkSync } from "node:fs";
import { randomUUID } from "node:crypto";
import type { Json, Rpc } from "./rpc.ts";

export type Connection = Pick<Rpc, "call" | "close" | "onAsk" | "listeners" | "exited">;
type Thread = { rpc: Connection; session: string };
type Entry = { opening?: Promise<Thread>; rpc?: Connection; users: number; timer?: ReturnType<typeof setTimeout> };
type Limits = { maxThreads?: number; idleMs?: number; onRetire?: (key: string) => void };

/** Own a connection per thread so extension questions always have one recipient. */
export class Threads {
  private paths = new Map<string, string>();
  private live = new Map<string, Entry>();
  private closed = false;
  private maxThreads: number;
  private idleMs: number;
  private onRetire: (key: string) => void;
  private connections = new Set<Connection>();
  private state: string;
  private connect: () => Connection;
  private defaults: Json;
  private onAsk: (key: string, rpc: Connection, ask: Json) => void;

  constructor(state: string, connect: () => Connection, defaults: Json,
              onAsk: (key: string, rpc: Connection, ask: Json) => void, limits: Limits = {}) {
    this.maxThreads = limits.maxThreads ?? 16;
    this.idleMs = limits.idleMs ?? 15 * 60 * 1000;
    this.onRetire = limits.onRetire ?? (() => {});
    for (const value of [this.maxThreads, this.idleMs]) {
      if (!Number.isSafeInteger(value) || value <= 0) throw new Error("Thread limits must be positive integers");
    }
    if (this.idleMs > 2_147_483_647) throw new Error("E_SLACK_IDLE_MS exceeds the timer limit (2147483647)");
    this.state = state;
    this.connect = connect;
    this.defaults = defaults;
    this.onAsk = onAsk;
    try {
      const saved = JSON.parse(readFileSync(state, "utf8")) as Record<string, { path?: string }>;
      if (!saved || typeof saved !== "object" || Array.isArray(saved)) {
        throw new Error("Slack state must be a map of thread paths");
      }
      for (const [key, thread] of Object.entries(saved)) {
        if (!thread || typeof thread !== "object" || Array.isArray(thread) ||
            (thread.path != null && typeof thread.path !== "string")) {
          throw new Error(`Invalid saved Slack thread: ${key}`);
        }
        // Older maps also stored session IDs. They belong to a dead process.
        if (typeof thread.path === "string") this.paths.set(key, thread.path);
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
  }

  has(key: string): boolean {
    return this.live.has(key) || this.paths.has(key);
  }

  /** Concurrent first messages share the same connection and resume operation. */
  get(key: string, name: string): Promise<Thread> {
    if (this.closed) return Promise.reject(new Error("Slack sessions are shutting down"));
    const known = this.live.get(key);
    if (known) {
      this.idle(key, known);
      return known.opening!;
    }
    if (this.connections.size >= this.maxThreads) {
      return Promise.reject(new Error("Slack session limit reached; wait for an idle thread to close"));
    }
    const entry: Entry = { users: 0 };
    this.live.set(key, entry);
    entry.opening = this.open(key, name, entry);
    return entry.opening;
  }

  /** A running turn pins its process; other threads can still run independently. */
  async use<T>(key: string, name: string, work: (thread: Thread) => Promise<T>): Promise<T> {
    const opening = this.get(key, name);
    const entry = this.live.get(key);
    if (!entry) return opening.then(work);
    if (entry.users) throw new Error("This Slack thread already has a running turn; send stop to interrupt it");
    entry.users++;
    clearTimeout(entry.timer);
    try {
      return await work(await opening);
    } finally {
      entry.users--;
      this.idle(key, entry);
    }
  }

  private idle(key: string, entry: Entry) {
    clearTimeout(entry.timer);
    if (entry.users || !entry.rpc || this.live.get(key) !== entry) return;
    entry.timer = setTimeout(() => { void this.retire(key, entry, true); }, this.idleMs);
    entry.timer.unref();
  }

  private async retire(key: string, entry: Entry, close: boolean) {
    if (this.live.get(key) !== entry) return;
    this.live.delete(key);
    clearTimeout(entry.timer);
    this.onRetire(key);
    if (entry.rpc) {
      if (close) await entry.rpc.close();
      this.connections.delete(entry.rpc);
    }
  }

  private async open(key: string, name: string, entry: Entry): Promise<Thread> {
    try {
      const rpc = this.connect();
      entry.rpc = rpc;
      this.connections.add(rpc);
      void rpc.exited.then(() => this.retire(key, entry, false));
      rpc.onAsk = (ask) => this.onAsk(key, rpc, ask);
      await rpc.call("hello", { ask: true });
      const path = this.paths.get(key);
      const created = await rpc.call("session.create", {
        ...this.defaults, save: true, name, ...(path ? { resume: path } : {}),
      });
      if (this.live.get(key) !== entry) throw new Error("Slack session closed while opening");
      if (typeof created.path === "string") this.save(key, created.path);
      this.idle(key, entry);
      return { rpc, session: String(created.session) };
    } catch (error) {
      await this.retire(key, entry, true);
      throw error;
    }
  }

  /** Persist paths only; the next process must reopen each conversation. */
  save(key: string, path: string) {
    const paths = new Map(this.paths).set(key, path);
    const saved = Object.fromEntries([...paths].map(([key, path]) => [key, { path }]));
    const temporary = `${this.state}.${randomUUID()}.tmp`;
    try {
      writeFileSync(temporary, JSON.stringify(saved, null, 2), { flag: "wx", mode: 0o600, flush: true });
      renameSync(temporary, this.state);
      this.paths = paths;
    } finally {
      try { unlinkSync(temporary); } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
      }
    }
  }

  async close() {
    this.closed = true;
    for (const [key, entry] of this.live) {
      clearTimeout(entry.timer);
      this.onRetire(key);
    }
    this.live.clear();
    // Include connections still waiting for hello or session.create.
    await Promise.all([...this.connections].map((rpc) => rpc.close()));
    this.connections.clear();
  }
}
