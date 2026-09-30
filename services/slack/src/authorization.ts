/** Every Slack entry point shares the same explicit user and channel policy. */
import type { AnyMiddlewareArgs, Middleware } from "@slack/bolt";

function ids(value: string | undefined, name: string, pattern: RegExp): Set<string> {
  const values = (value ?? "").split(",").map((id) => id.trim());
  if (!values.length || values.some((id) => !pattern.test(id))) {
    throw new Error(`${name} must contain comma-separated Slack IDs`);
  }
  return new Set(values);
}

/** Denied actions are acknowledged but never reach a prompt or an approval handler. */
export function authorization(users: string | undefined, channels: string | undefined): Middleware<AnyMiddlewareArgs> {
  const allowedUsers = ids(users, "E_SLACK_ALLOWED_USERS", /^[UW][A-Z0-9]+$/);
  const allowedChannels = ids(channels, "E_SLACK_ALLOWED_CHANNELS", /^[CG][A-Z0-9]+$/);
  return async (args) => {
    const body = args.body as unknown as Record<string, unknown>;
    const event = body.event as Record<string, unknown> | undefined;
    const actor = body.user as { id?: string } | undefined;
    const channel = body.channel as { id?: string } | undefined;
    const userId = event?.user ?? actor?.id;
    const channelId = event?.channel ?? channel?.id;
    if (typeof userId === "string" && typeof channelId === "string" &&
        !event?.bot_id && !event?.subtype &&
        allowedUsers.has(userId) && allowedChannels.has(channelId)) {
      await args.next();
    } else if ("ack" in args && args.ack) {
      await args.ack();
    }
  };
}
