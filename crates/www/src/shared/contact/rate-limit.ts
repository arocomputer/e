/**
 * Rate limits count in the RateLimiter Durable Object when the Worker passes
 * its binding, and in per-instance memory otherwise (local runs and tests).
 */
import type { RateLimiter } from "./rate-limiter";

type Options = {
  /** Bucket name, so different endpoints never share a counter. */
  name: string;
  /** Caller identity, usually the client IP. */
  id: string;
  max: number;
  windowMs: number;
};

/** Compute the fixed time window and its remaining retry delay. */
function bucketFor(windowMs: number) {
  const now = Date.now();
  const index = Math.floor(now / windowMs);
  return {
    index,
    retryAfter: Math.ceil(((index + 1) * windowMs - now) / 1000),
  };
}

const GLOBAL_MULTIPLIER = 12;

const counters = new Map<string, number>();
let memoBucket = -1;

/** Limit one instance by caller and total request count; reset each window. */
function memoryLimit({ name, id, max, windowMs }: Options) {
  const { index, retryAfter } = bucketFor(windowMs);

  // Clear the previous window to bound memory use.
  if (index !== memoBucket) {
    counters.clear();
    memoBucket = index;
  }

  const globalKey = `${name}:*`;
  const total = (counters.get(globalKey) ?? 0) + 1;
  counters.set(globalKey, total);
  if (total > max * GLOBAL_MULTIPLIER) return retryAfter;

  const key = `${name}:${id}`;
  const count = (counters.get(key) ?? 0) + 1;
  counters.set(key, count);
  return count > max ? retryAfter : null;
}

/** Return a retry delay in seconds, or null. Limiter errors are logged and allow the request. */
export async function rateLimit(
  options: Options,
  limiter?: DurableObjectNamespace<RateLimiter>,
): Promise<number | null> {
  if (!limiter) return memoryLimit(options);
  try {
    return await limiter
      .getByName(`${options.name}:${options.id}`)
      .hit(options.max, options.windowMs);
  } catch (err) {
    console.error("rate-limit: limiter unavailable, failing open:", err);
    return null;
  }
}
