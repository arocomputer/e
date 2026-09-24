import { DurableObject } from "cloudflare:workers";

/**
 * A per-caller attempt counter that holds across every Worker instance: one
 * Durable Object per key (`contact:<ip>`), reached with
 * `env.RATE_LIMITER.getByName(key).hit(max, windowMs)`. The window opens at
 * the first attempt, and an alarm deletes the object's storage when it closes.
 */
export class RateLimiter extends DurableObject {
  /** Count one attempt; return seconds until the window closes when over `max`, otherwise null. */
  async hit(max: number, windowMs: number): Promise<number | null> {
    const now = Date.now();
    const stored = await this.ctx.storage.get<{
      count: number;
      until: number;
    }>("window");
    const open = stored && stored.until > now;
    const window = open ? stored : { count: 0, until: now + windowMs };
    window.count += 1;
    await this.ctx.storage.put("window", window);
    if (!open) await this.ctx.storage.setAlarm(window.until);
    return window.count > max ? Math.ceil((window.until - now) / 1000) : null;
  }

  /** The window closed: forget the caller. */
  async alarm() {
    await this.ctx.storage.deleteAll();
  }
}
