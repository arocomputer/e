/** Live RPC processes belong to threads; only saved log paths survive a restart. */
import { readFileSync, writeFileSync } from "node:fs";
import type { Json, Rpc } from "./rpc.ts";

export type Connection = Pick<Rpc, "call" | "close" | "onAsk" | "listeners">;
type Thread = { rpc: Connection; session: string };

/** Own a connection per thread so extension questions always have one recipient. */
export class Threads {
  private paths = new Map<string, string>();
  private live = new Map<string, Promise<Thread>>();
  private state: string;
  private connect: () => Connection;
  private defaults: Json;
  private onAsk: (key: string, rpc: Connection, ask: Json) => void;

  constructor(state: string, connect: () => Connection, defaults: Json,
              onAsk: (key: string, rpc: Connection, ask: Json) => void) {
    this.state = state;
    this.connect = connect;
    this.defaults = defaults;
    this.onAsk = onAsk;
    try {
      const saved = JSON.parse(readFileSync(state, "utf8")) as Record<string, { path?: string }>;
      for (const [key, thread] of Object.entries(saved)) {
        // Older maps also stored session IDs. They belong to a dead process.
        if (typeof thread?.path === "string") this.paths.set(key, thread.path);
      }
    } catch {
      // A missing state file starts with no saved conversations.
    }
  }

  has(key: string): boolean {
    return this.live.has(key) || this.paths.has(key);
  }

  /** Concurrent first messages share the same connection and resume operation. */
  get(key: string, name: string): Promise<Thread> {
    const known = this.live.get(key);
    if (known) return known;
    const opening = this.open(key, name).catch((error) => {
      this.live.delete(key);
      throw error;
    });
    this.live.set(key, opening);
    return opening;
  }

  private async open(key: string, name: string): Promise<Thread> {
    const rpc = this.connect();
    rpc.onAsk = (ask) => this.onAsk(key, rpc, ask);
    try {
      await rpc.call("hello", { ask: true });
      const path = this.paths.get(key);
      const created = await rpc.call("session.create", {
        ...this.defaults, save: true, name, ...(path ? { resume: path } : {}),
      });
      if (typeof created.path === "string") this.save(key, created.path);
      return { rpc, session: String(created.session) };
    } catch (error) {
      await rpc.close();
      throw error;
    }
  }

  /** Persist paths only; the next process must reopen each conversation. */
  save(key: string, path: string) {
    this.paths.set(key, path);
    const saved = Object.fromEntries([...this.paths].map(([key, path]) => [key, { path }]));
    writeFileSync(this.state, JSON.stringify(saved, null, 2));
  }

  async close() {
    await Promise.all([...this.live.values()].map(async (opening) => {
      const thread = await opening.catch(() => undefined);
      await thread?.rpc.close();
    }));
    this.live.clear();
  }
}
