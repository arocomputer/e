/** Prepare and send recipient-specific marketing email with signed unsubscribe links. */
import { unsubscribeHeaders, unsubscribeUrl } from "./unsubscribe.ts";

const RESEND_API_KEY = process.env.RESEND_API_KEY;
const FROM = process.env.MARKETING_FROM || process.env.CONTACT_FROM;

const REPLY_TO = process.env.MARKETING_REPLY_TO;

const POSTAL_ADDRESS = process.env.MARKETING_POSTAL_ADDRESS;

/** Replaced with the recipient's signed unsubscribe URL. Required in the body. */
export const UNSUBSCRIBE_PLACEHOLDER = "{{unsubscribe_url}}";

/** Replaced with MARKETING_POSTAL_ADDRESS. Required in the body. */
export const POSTAL_PLACEHOLDER = "{{postal_address}}";

export type MarketingEmail = {
  to: string;
  subject: string;
  /** At least one of html/text. Each must contain BOTH placeholders. */
  html?: string;
  text?: string;
};

export type SendResult =
  | { ok: true; id: string | null }
  | {
      ok: false;
      reason:
        "not_configured" | "missing_unsubscribe" | "missing_postal" | "failed";
      detail?: string;
    };

/** Substitute the signed opt-out link and sender address in one message body. */
function inject(body: string | undefined, url: string, postal: string) {
  if (body === undefined) return undefined;
  return body
    .split(UNSUBSCRIBE_PLACEHOLDER)
    .join(url)
    .split(POSTAL_PLACEHOLDER)
    .join(postal);
}

type PreparedEmail = {
  from: string;
  reply_to?: string;
  to: string[];
  subject: string;
  html?: string;
  text?: string;
  headers: Record<string, string>;
};

type Preparation =
  { ok: true; payload: PreparedEmail } | Extract<SendResult, { ok: false }>;

/** Validate configuration and render one message without making a network request. */
export function prepareMarketingEmail(email: MarketingEmail): Preparation {
  if (!RESEND_API_KEY || !FROM) {
    console.error(
      `Marketing send: ${!RESEND_API_KEY ? "RESEND_API_KEY" : "MARKETING_FROM/CONTACT_FROM"} is not set.`,
    );
    return { ok: false, reason: "not_configured" };
  }
  if (!POSTAL_ADDRESS) {
    console.error(
      "Marketing send refused: MARKETING_POSTAL_ADDRESS is not set.",
    );
    return {
      ok: false,
      reason: "missing_postal",
      detail: "MARKETING_POSTAL_ADDRESS unset",
    };
  }

  const bodies = [email.html, email.text].filter(
    (b): b is string => typeof b === "string",
  );
  if (bodies.length === 0) {
    return {
      ok: false,
      reason: "missing_unsubscribe",
      detail: "no html or text body",
    };
  }
  // Fail closed: every provided body must carry the visible opt-out.
  if (!bodies.every((b) => b.includes(UNSUBSCRIBE_PLACEHOLDER))) {
    console.error(
      `Marketing send refused: every body must contain ${UNSUBSCRIBE_PLACEHOLDER}.`,
    );
    return { ok: false, reason: "missing_unsubscribe" };
  }
  // Every supplied body must include the sender address.
  if (!bodies.every((b) => b.includes(POSTAL_PLACEHOLDER))) {
    console.error(
      `Marketing send refused: every body must contain ${POSTAL_PLACEHOLDER}.`,
    );
    return { ok: false, reason: "missing_postal" };
  }

  // Require a signed link before preparing a message.
  let url: string;
  let headers: Record<string, string>;
  try {
    url = unsubscribeUrl(email.to);
    headers = unsubscribeHeaders(email.to);
  } catch (err) {
    console.error("Marketing send: cannot sign unsubscribe token:", err);
    return { ok: false, reason: "not_configured" };
  }

  return {
    ok: true,
    payload: {
      from: FROM,
      ...(REPLY_TO ? { reply_to: REPLY_TO } : {}),
      to: [email.to],
      subject: email.subject,
      html: inject(email.html, url, POSTAL_ADDRESS),
      text: inject(email.text, url, POSTAL_ADDRESS),
      headers,
    },
  };
}

/** Send a validated message. Provider errors return a result; network failures reject. */
export async function sendMarketingEmail(
  email: MarketingEmail,
): Promise<SendResult> {
  const prepared = prepareMarketingEmail(email);
  if (!prepared.ok) return prepared;
  const res = await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${RESEND_API_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(prepared.payload),
  });

  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    console.error("Resend marketing send error:", res.status, detail);
    return { ok: false, reason: "failed", detail };
  }

  const body = (await res.json().catch(() => null)) as { id?: string } | null;
  return { ok: true, id: body?.id ?? null };
}
