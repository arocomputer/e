/**
 * e for Slack: one thread, one e session, over a spawned `e rpc`.
 *
 * Two halves. `Rpc` speaks the JSONL protocol (docs/automation.md): it
 * writes requests, resolves their responses by id, and routes event lines
 * to whoever owns the session they name. The Bolt app maps Slack threads
 * to sessions and turns events into messages. Nothing else — copy this and
 * change what your team wants posted.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import { readFileSync, writeFileSync } from "node:fs";
import bolt from "@slack/bolt";

const { App } = bolt;

type Json = Record<string, unknown>;

// ---------------------------------------------------------------- e rpc

/** A spawned `e rpc` and the pipes to it. */
class Rpc {
  private child: ChildProcess;
  private next = 1;
  private pending = new Map<string, { resolve: (v: Json) => void; reject: (e: Error) => void }>();
  /** Event lines by session id; a turn's owner registers here. */
  readonly listeners = new Map<string, (event: Json) => void>();
  /** `ask` lines: an extension's question for a person. */
  onAsk: (ask: Json) => void = () => {};

  constructor(bin: string, args: string[]) {
    this.child = spawn(bin, [...args, "rpc"], { stdio: ["pipe", "pipe", "inherit"] });
    createInterface({ input: this.child.stdout! }).on("line", (line) => this.receive(line));
    this.child.on("exit", (code) => {
      for (const p of this.pending.values()) p.reject(new Error(`e rpc exited (${code})`));
      this.pending.clear();
    });
  }

  private receive(line: string) {
    let msg: Json;
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    if (typeof msg.type === "string") {
      if (msg.type === "ask") this.onAsk(msg);
      else if (typeof msg.session === "string") this.listeners.get(msg.session)?.(msg);
      return;
    }
    const p = this.pending.get(String(msg.id));
    if (!p) return;
    this.pending.delete(String(msg.id));
    if (msg.error) p.reject(new Error(String(msg.error)));
    else p.resolve((msg.result ?? {}) as Json);
  }

  /** One request, one response. A prompt resolves when its turn ends. */
  call(method: string, params: Json = {}): Promise<Json> {
    const id = `r${this.next++}`;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.child.stdin!.write(JSON.stringify({ id, method, params }) + "\n");
    });
  }

  async close() {
    try {
      await this.call("shutdown");
    } catch {
      // Already gone.
    }
  }
}

// ---------------------------------------------------------------- state

/** Thread → session. Saved so a restart resumes old threads from disk. */
interface Thread {
  session?: string;
  path?: string;
}

const STATE = process.env.E_SLACK_STATE ?? "./e-slack-state.json";
const threads = new Map<string, Thread>(loadState());

function loadState(): [string, Thread][] {
  try {
    return Object.entries(JSON.parse(readFileSync(STATE, "utf8")));
  } catch {
    return [];
  }
}

function saveState() {
  writeFileSync(STATE, JSON.stringify(Object.fromEntries(threads), null, 2));
}

// ---------------------------------------------------------------- app

const env = (name: string) => {
  const v = process.env[name];
  if (!v) throw new Error(`${name} is required (see .env.example)`);
  return v;
};

const cwd = env("E_CWD");
const rpc = new Rpc(process.env.E_BIN ?? "e", []);
const hello = await rpc.call("hello", { ask: true });
console.log(`e ${hello.version} (${hello.channel}), protocol ${hello.protocol}`);

const app = new App({
  token: env("SLACK_BOT_TOKEN"),
  signingSecret: env("SLACK_SIGNING_SECRET"),
  appToken: env("SLACK_APP_TOKEN"),
  socketMode: true,
});

/** The session for a thread, creating or resuming it. */
async function sessionFor(key: string, name: string): Promise<string> {
  const known = threads.get(key);
  if (known?.session) return known.session;
  const params: Json = { cwd, save: true, name };
  if (process.env.E_MODEL) params.model = process.env.E_MODEL;
  if (known?.path) params.resume = known.path;
  const created = await rpc.call("session.create", params);
  const session = String(created.session);
  threads.set(key, { session, path: known?.path });
  saveState();
  return session;
}

/** Text a person wants to read about a finished tool call. */
function toolLine(batch: Map<number, Json>, end: Json): string | null {
  const call = batch.get(Number(end.id));
  if (!call) return null;
  const target = call.target ? ` \`${call.target}\`` : "";
  const failed = end.outcome !== "completed" ? " — failed" : "";
  return `▸ ${call.name}${target}${failed}`;
}

/** Slack messages cap near 4000 characters; split long replies on lines. */
function chunks(text: string, size = 3900): string[] {
  const out: string[] = [];
  let rest = text;
  while (rest.length > size) {
    let cut = rest.lastIndexOf("\n", size);
    if (cut < size / 2) cut = size;
    out.push(rest.slice(0, cut));
    rest = rest.slice(cut).replace(/^\n/, "");
  }
  out.push(rest);
  return out;
}

interface Post {
  (text: string, blocks?: unknown[]): Promise<unknown>;
}

/** Run one prompt on a thread's session, posting as it goes. */
async function runTurn(key: string, name: string, prompt: string, post: Post) {
  const session = await sessionFor(key, name);
  const batch = new Map<number, Json>();
  rpc.listeners.set(session, (event) => {
    switch (event.type) {
      case "tool_batch":
        for (const call of event.calls as Json[]) batch.set(Number(call.id), call);
        break;
      case "tool_end": {
        const line = toolLine(batch, event);
        if (line) void post(line);
        break;
      }
      case "error":
        void post(`:warning: ${event.message}`);
        break;
    }
  });
  try {
    const result = await rpc.call("session.prompt", { session, prompt });
    if (result.error) {
      await post(`:x: ${result.error}`);
      return;
    }
    if (result.aborted) {
      await post("_stopped_");
      return;
    }
    const text = String(result.final_output || "_(no reply)_");
    for (const part of chunks(text)) await post(part);
    const cost = result.cost_usd;
    if (typeof cost === "number") await post(`_$${cost.toFixed(4)}_`);
    const created = await rpc.call("session.info", { session });
    const thread = threads.get(key);
    if (thread && created.path) {
      thread.path = String(created.path);
      saveState();
    }
  } catch (error) {
    await post(`:x: ${(error as Error).message}`);
  } finally {
    rpc.listeners.delete(session);
  }
}

// A message in a thread while an extension waits for text answers it.
const textAsks = new Map<string, number>();

rpc.onAsk = (ask) => {
  const n = Number(ask.ask);
  const params = (ask.params ?? {}) as Json;
  const title = String(params.title ?? ask.method);
  // Which thread asked? The extension host does not say; the most recent
  // turn's thread is the best single-thread answer. Multi-thread deployments
  // should key sessions to threads here.
  const key = lastThread;
  if (!key) {
    void rpc.call("ask.reply", { ask: n });
    return;
  }
  const [channel, thread_ts] = key.split(":");
  const post = (text: string, blocks?: unknown[]) =>
    app.client.chat.postMessage({ channel, thread_ts, text, blocks: blocks as never });
  switch (ask.method) {
    case "ui.confirm":
      void post(title, [
        { type: "section", text: { type: "mrkdwn", text: `*${title}*\n${params.message ?? ""}` } },
        {
          type: "actions",
          elements: [
            { type: "button", text: { type: "plain_text", text: "Yes" }, style: "primary", action_id: "ask_yes", value: String(n) },
            { type: "button", text: { type: "plain_text", text: "No" }, action_id: "ask_no", value: String(n) },
          ],
        },
      ]);
      break;
    case "ui.select": {
      const options = ((params.options ?? []) as (string | Json)[]).slice(0, 5).map((o) => {
        const label = typeof o === "string" ? o : String(o.label);
        const value = typeof o === "string" ? o : String(o.value ?? o.label);
        return {
          type: "button",
          text: { type: "plain_text", text: label.slice(0, 75) },
          action_id: `ask_pick_${value}`,
          value: JSON.stringify({ n, value, label }),
        };
      });
      void post(title, [
        { type: "section", text: { type: "mrkdwn", text: `*${title}*` } },
        { type: "actions", elements: options },
      ]);
      break;
    }
    default:
      // ui.input / ui.editor: the next message in the thread is the answer.
      textAsks.set(key, n);
      void post(`*${title}* — reply in this thread${params.placeholder ? ` (${params.placeholder})` : ""}`);
  }
};

let lastThread: string | null = null;

app.action("ask_yes", async ({ ack, action }) => {
  await ack();
  await rpc.call("ask.reply", { ask: Number((action as unknown as Json).value), result: { confirmed: true } });
});
app.action("ask_no", async ({ ack, action }) => {
  await ack();
  await rpc.call("ask.reply", { ask: Number((action as unknown as Json).value), result: { confirmed: false } });
});
app.action(/^ask_pick_/, async ({ ack, action }) => {
  await ack();
  const { n, value, label } = JSON.parse(String((action as unknown as Json).value));
  await rpc.call("ask.reply", { ask: n, result: { value, label } });
});

function stripMention(text: string): string {
  return text.replace(/<@[A-Z0-9]+>/g, "").trim();
}

app.event("app_mention", async ({ event, client }) => {
  const channel = event.channel;
  const thread_ts = event.thread_ts ?? event.ts;
  const key = `${channel}:${thread_ts}`;
  lastThread = key;
  const post: Post = (text, blocks) =>
    client.chat.postMessage({ channel, thread_ts, text, blocks: blocks as never });
  const prompt = stripMention(event.text ?? "");
  if (!prompt) {
    await post("Ask me something in this thread.");
    return;
  }
  await runTurn(key, `slack ${channel}/${thread_ts}`, prompt, post);
});

app.message(async ({ message, client }) => {
  // Thread replies only, from people, in threads we know.
  const m = message as unknown as Json;
  if (m.subtype || !m.thread_ts || m.thread_ts === m.ts) return;
  const channel = String(m.channel);
  const thread_ts = String(m.thread_ts);
  const key = `${channel}:${thread_ts}`;
  const known = threads.get(key);
  if (!known) return;
  const text = stripMention(String(m.text ?? ""));
  if (!text) return;
  lastThread = key;
  const pendingAsk = textAsks.get(key);
  if (pendingAsk !== undefined) {
    textAsks.delete(key);
    await rpc.call("ask.reply", { ask: pendingAsk, result: { text } });
    return;
  }
  const post: Post = (t, blocks) => client.chat.postMessage({ channel, thread_ts, text: t, blocks: blocks as never });
  if (text.toLowerCase() === "stop" && known.session) {
    await rpc.call("session.interrupt", { session: known.session });
    return;
  }
  await runTurn(key, `slack ${channel}/${thread_ts}`, text, post);
});

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, async () => {
    await rpc.close();
    process.exit(0);
  });
}

await app.start();
console.log("e for Slack is listening");
