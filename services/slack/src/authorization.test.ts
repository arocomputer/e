import assert from "node:assert/strict";
import { test } from "node:test";
import { authorization } from "./authorization.ts";

test("authorization refuses empty, wildcard, or malformed configuration", () => {
  for (const users of [undefined, "", "*", "U123,", "someone@example.com"]) {
    assert.throws(() => authorization(users, "C123"), /E_SLACK_ALLOWED_USERS/);
  }
  for (const channels of [undefined, "", "*", "D123", "general"]) {
    assert.throws(() => authorization("U123", channels), /E_SLACK_ALLOWED_CHANNELS/);
  }
});

test("mentions, replies, and buttons require both an allowed user and channel", async () => {
  const authorize = authorization(" U123, W456 ", "C123,G456");
  for (const kind of ["app_mention", "message", "block_actions"]) {
    for (const user of ["U123", "U999", undefined]) {
      for (const channel of ["C123", "C999", undefined]) {
        let ran = false, acked = false;
        const body = kind === "block_actions"
          ? { type: kind, user: { id: user }, channel: { id: channel } }
          : { event: { type: kind, user, channel } };
        await authorize({ body, next: async () => { ran = true; },
          ...(kind === "block_actions" ? { ack: async () => { acked = true; } } : {}),
        } as Parameters<typeof authorize>[0]);
        const allowed = user === "U123" && channel === "C123";
        assert.equal(ran, allowed, JSON.stringify(body));
        assert.equal(acked, kind === "block_actions" && !allowed);
      }
    }
  }
});

test("bot messages cannot use an authorized user's identity", async () => {
  const authorize = authorization("U123", "C123");
  for (const extra of [{ bot_id: "B123" }, { subtype: "bot_message" }]) {
    let ran = false;
    await authorize({ body: { event: { user: "U123", channel: "C123", ...extra } },
      next: async () => { ran = true; },
    } as Parameters<typeof authorize>[0]);
    assert.equal(ran, false);
  }
});
