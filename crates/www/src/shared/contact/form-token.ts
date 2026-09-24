/** Signed timestamps for contact submissions. Tokens must be between three seconds and six hours old. */
import { Buffer } from "node:buffer";
import { createHmac, timingSafeEqual } from "node:crypto";

// A dedicated form key takes precedence over the unsubscribe key.
const SECRET = process.env.FORM_TOKEN_SECRET || process.env.UNSUBSCRIBE_SECRET;

const DOMAIN = "contact-form:v1:";

/** Minimum age accepted for a submitted form token. */
export const MIN_AGE_MS = 3_000;

/** Expiry window for an open contact form. */
export const MAX_AGE_MS = 6 * 60 * 60 * 1000;

/** Whether the handler can require signed timing tokens. */
export const isConfigured = Boolean(SECRET);

/** Sign the timestamp with a contact-specific prefix. */
function sign(payload: string): string {
  if (!SECRET) throw new Error("no form token secret configured");
  return createHmac("sha256", SECRET)
    .update(DOMAIN + payload)
    .digest("base64url");
}

/** Compare equal-length signatures without content-dependent timing. */
function safeEqual(givenRaw: string, expectedRaw: string) {
  const given = Buffer.from(givenRaw);
  const expected = Buffer.from(expectedRaw);
  return given.length === expected.length && timingSafeEqual(given, expected);
}

/** Mint a token stamped with the current time. */
export function mintFormToken(): string {
  const issued = String(Date.now());
  return `${Buffer.from(issued).toString("base64url")}.${sign(issued)}`;
}

export type TokenVerdict =
  | { ok: true }
  | { ok: false; reason: "missing" | "malformed" | "too_fast" | "expired" };

/**
 * Verify a token's signature and age. Constant-time comparison, and the age is
 * read from the signed payload rather than anything the client can set.
 */
export function verifyFormToken(
  token: unknown,
  now = Date.now(),
): TokenVerdict {
  if (typeof token !== "string" || !token)
    return { ok: false, reason: "missing" };

  const dot = token.indexOf(".");
  if (dot < 1) return { ok: false, reason: "malformed" };

  const issuedRaw = Buffer.from(token.slice(0, dot), "base64url").toString(
    "utf8",
  );
  if (!/^\d+$/.test(issuedRaw)) return { ok: false, reason: "malformed" };

  let expected: string;
  try {
    expected = sign(issuedRaw);
  } catch {
    return { ok: false, reason: "malformed" };
  }
  if (!safeEqual(token.slice(dot + 1), expected))
    return { ok: false, reason: "malformed" };

  const age = now - Number(issuedRaw);
  // A negative age means a clock skew or a forged-but-correctly-signed future
  // stamp; treat it as too fast rather than letting it through.
  if (age < MIN_AGE_MS) return { ok: false, reason: "too_fast" };
  if (age > MAX_AGE_MS) return { ok: false, reason: "expired" };

  return { ok: true };
}
