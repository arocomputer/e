/** Sign unsubscribe links and receipts, and update the shared Resend contact list. */
import { Buffer } from "node:buffer";
import { createHmac, timingSafeEqual } from "node:crypto";

const SITE_URL = (process.env.SITE_URL || "https://aro.computer").replace(
  /\/+$/,
  "",
);
const SECRET = process.env.UNSUBSCRIBE_SECRET;
const RESEND_API_KEY =
  process.env.RESEND_CONTACTS_API_KEY || process.env.RESEND_API_KEY;

/** Normalize the address used in signed tokens and provider updates. */
const normalize = (email: string) => email.trim().toLowerCase();

/** Sign an unsubscribe payload; unsigned links are never issued. */
function sign(payload: string): string {
  if (!SECRET) throw new Error("UNSUBSCRIBE_SECRET is not set");
  return createHmac("sha256", SECRET).update(payload).digest("base64url");
}

/** Compare equal-length signatures in constant time. */
function safeEqual(givenRaw: string, expectedRaw: string) {
  const given = Buffer.from(givenRaw);
  const expected = Buffer.from(expectedRaw);
  return given.length === expected.length && timingSafeEqual(given, expected);
}

/** Mint a tamper-proof unsubscribe token for an email address. */
export function signUnsubToken(email: string): string {
  const payload = normalize(email);
  return `${Buffer.from(payload).toString("base64url")}.${sign(payload)}`;
}

/** Bind the success receipt to a completed unsubscribe token. Receipts do not expire. */
export function signUnsubReceipt(token: string): string {
  return sign(`unsubscribe:done:${token}`);
}

/** Accept a success receipt only for its signed unsubscribe token. */
export function verifyUnsubReceipt(
  token: string | null | undefined,
  receipt: string | null | undefined,
): boolean {
  if (!token || !receipt || !SECRET) return false;
  try {
    return safeEqual(receipt, signUnsubReceipt(token));
  } catch {
    return false;
  }
}

/** The email a token was minted for, or null if it's missing, forged, or the
 *  signing secret isn't configured. Constant-time comparison. */
export function verifyUnsubToken(
  token: string | null | undefined,
): string | null {
  if (!token || !SECRET) return null;
  const dot = token.indexOf(".");
  if (dot < 1) return null;
  const email = Buffer.from(token.slice(0, dot), "base64url").toString("utf8");
  let expected: string;
  try {
    expected = sign(email);
  } catch {
    return null;
  }
  if (!safeEqual(token.slice(dot + 1), expected)) return null;
  return email;
}

export type UnsubResult =
  { ok: true } | { ok: false; reason: "not_configured" | "failed" };

/** Set the contact to unsubscribed globally in Resend (team-wide opt-out). */
export async function unsubscribeEmail(email: string): Promise<UnsubResult> {
  if (!RESEND_API_KEY) return { ok: false, reason: "not_configured" };
  const res = await fetch(
    `https://api.resend.com/contacts/${encodeURIComponent(normalize(email))}`,
    {
      method: "PATCH",
      headers: {
        Authorization: `Bearer ${RESEND_API_KEY}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ unsubscribed: true }),
    },
  );
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    console.error("Resend unsubscribe error:", res.status, detail);
    return { ok: false, reason: "failed" };
  }
  return { ok: true };
}

/** Link from an email body to the branded unsubscribe page. */
export function unsubscribeUrl(email: string): string {
  return `${SITE_URL}/unsubscribe?token=${encodeURIComponent(signUnsubToken(email))}`;
}

/** RFC 8058 one-click headers to attach when sending bulk/marketing email, so
 *  Gmail/Apple Mail render a native Unsubscribe button that POSTs to our API. */
export function unsubscribeHeaders(email: string): Record<string, string> {
  const token = encodeURIComponent(signUnsubToken(email));
  return {
    "List-Unsubscribe": `<${SITE_URL}/api/unsubscribe?token=${token}>`,
    "List-Unsubscribe-Post": "List-Unsubscribe=One-Click",
  };
}

/** Mask an address for display: jane.doe@aro.computer -> j••••ulo@aro.computer */
export function maskEmail(email: string): string {
  const [user, domain] = normalize(email).split("@");
  if (!domain) return email;
  const shown =
    user.length <= 2
      ? user
      : `${user[0]}${"•".repeat(Math.min(user.length - 2, 6))}${user[user.length - 1]}`;
  return `${shown}@${domain}`;
}
