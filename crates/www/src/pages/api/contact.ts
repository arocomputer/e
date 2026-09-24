import type { APIContext } from "astro";
import { env } from "cloudflare:workers";
import { rateLimit } from "@/shared/contact/rate-limit";
import { isConfigured, verifyFormToken } from "@/shared/contact/form-token";
import { subscribeContact } from "@/shared/email/subscribers";
import { validateNote, type ContactNote } from "@/shared/contact/note";

// Delivers contact-form submissions as email via Resend (https://resend.com).
// Configuration lives in Worker secrets and vars — see docs/contact.md. If the
// key is absent the route fails cleanly with a 503 rather than pretending to send.
export const prerender = false;
const RESEND_API_KEY = process.env.RESEND_API_KEY;
const CONTACT_TO = process.env.CONTACT_TO || "contact@aro.computer";
// No fallback: a missing CONTACT_FROM must 503 like a missing key, not
// silently send from Resend's sandbox domain (which masks misconfiguration).
const CONTACT_FROM = process.env.CONTACT_FROM;

// Limit the request before parsing; validateNote also caps every editable field.
const MAX_BODY_BYTES = 25_000;
const RATE_LIMIT_WINDOW_MS = 10 * 60 * 1000;
const RATE_LIMIT_MAX = 5;

// Collapse to a single line so a value can't be smuggled into the subject header.
const oneLine = (s: string) => s.replace(/\s+/g, " ").trim();

/** Identify the caller by the connecting IP the Worker reports. */
function clientKey(context: APIContext) {
  try {
    return context.clientAddress || "unknown";
  } catch {
    return "unknown";
  }
}

/** Reject oversized or non-object JSON before contact validation. */
async function readLimitedJson(request: Request) {
  if (Number(request.headers.get("content-length") || 0) > MAX_BODY_BYTES) {
    return { error: "Request too large.", status: 413 as const };
  }

  const reader = request.body?.getReader();
  if (!reader) return { error: "Invalid request.", status: 400 as const };

  const decoder = new TextDecoder();
  let bytes = 0;
  let body = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      if (bytes > MAX_BODY_BYTES) {
        await reader.cancel().catch(() => undefined);
        return { error: "Request too large.", status: 413 as const };
      }
      body += decoder.decode(value, { stream: true });
    }
    body += decoder.decode();
    const data = JSON.parse(body) as unknown;
    if (!data || typeof data !== "object" || Array.isArray(data)) {
      return { error: "Invalid request.", status: 400 as const };
    }
    return { data: data as Record<string, unknown> };
  } catch {
    return { error: "Invalid request.", status: 400 as const };
  }
}

/** Validate and send a note, then record any explicit marketing consent. */
export async function POST(context: APIContext) {
  const { request } = context;
  const retryAfter = await rateLimit(
    {
      name: "contact",
      id: clientKey(context),
      max: RATE_LIMIT_MAX,
      windowMs: RATE_LIMIT_WINDOW_MS,
    },
    env.RATE_LIMITER,
  );
  if (retryAfter) {
    return Response.json(
      { error: "Too many contact attempts. Please try again later." },
      { status: 429, headers: { "Retry-After": String(retryAfter) } },
    );
  }

  const parsed = await readLimitedJson(request);
  if ("error" in parsed) {
    return Response.json({ error: parsed.error }, { status: parsed.status });
  }
  const data = parsed.data;

  // Honeypot: real users leave this hidden field empty. Pretend success.
  if (typeof data.company_website === "string" && data.company_website.trim()) {
    return Response.json({ ok: true });
  }

  // Timing check. A script that POSTs this endpoint directly never loaded the
  // page, so it has no signed token and cannot forge one. Unlike the honeypot
  // this does NOT fake success: a missing token is ambiguous (a real visitor's
  // token fetch could have failed), so the caller gets a recoverable error and
  // `retryToken`, which tells the form to mint a fresh one and let them resubmit.
  if (isConfigured) {
    const verdict = verifyFormToken(data.form_token);
    if (!verdict.ok) {
      console.warn(`Contact form: timing token rejected (${verdict.reason}).`);
      return Response.json(
        {
          error: "Please take a moment, then send again.",
          retryToken: true,
        },
        { status: 400 },
      );
    }
  }

  const note: ContactNote = {
    name: typeof data.name === "string" ? data.name.trim() : "",
    email: typeof data.email === "string" ? data.email.trim() : "",
    subject: typeof data.subject === "string" ? data.subject.trim() : "",
    comments: typeof data.comments === "string" ? data.comments.trim() : "",
  };
  const validation = validateNote(note);
  if (validation) return Response.json({ error: validation }, { status: 400 });

  if (!RESEND_API_KEY || !CONTACT_FROM) {
    console.error(
      `Contact form: ${!RESEND_API_KEY ? "RESEND_API_KEY" : "CONTACT_FROM"} is not set.`,
    );
    return Response.json(
      { error: "Email delivery isn't configured yet." },
      { status: 503 },
    );
  }

  const optedIn = data.subscribe === "yes" || data.subscribe === true;

  const text = [
    "New contact from aro.computer",
    "",
    `Name:      ${note.name}`,
    `Email:     ${note.email}`,
    `Subject:   ${oneLine(note.subject || "A note for Aro")}`,
    // Recorded in the notification too, so there's a durable record of consent
    // outside Resend — which matters if anyone ever asks why they're on the list.
    `Updates:   ${optedIn ? "YES — opted in to marketing" : "no"}`,
    "",
    "Message:",
    note.comments,
  ].join("\n");

  const res = await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${RESEND_API_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      from: CONTACT_FROM,
      to: [CONTACT_TO],
      reply_to: note.email,
      subject: oneLine(
        `Aro · ${note.name} · ${note.subject || "A note for Aro"}`,
      ),
      text,
    }),
  });

  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    console.error("Resend error:", res.status, detail);
    return Response.json(
      { error: "Could not send your message. Please try again." },
      { status: 502 },
    );
  }

  // Marketing opt-in, only if they ticked the box. Runs *after* the notification
  // has already sent, and its failure is swallowed: someone trying to reach a
  // human must not lose their message because the contacts API was unhappy.
  if (optedIn) {
    // A single name field does not tell us which parts are given or family names.
    const sub = await subscribeContact({ email: note.email });
    if (!sub.ok) {
      console.error(`Contact form: opt-in not recorded (${sub.reason}).`);
    }
  }

  return Response.json({ ok: true });
}

/** Every other method is refused with an empty 405; the form only POSTs. */
export const ALL = () => new Response(null, { status: 405 });
