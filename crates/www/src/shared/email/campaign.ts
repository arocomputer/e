/** Select explicitly subscribed contacts and send each message with request pacing. */
import { prepareMarketingEmail, sendMarketingEmail } from "./marketing.ts";

const LIST_API_KEY =
  process.env.RESEND_CONTACTS_API_KEY || process.env.RESEND_API_KEY;

// Leave room below the provider's two-requests-per-second limit.
const SEND_INTERVAL_MS = 600;

export type Contact = { email: string; firstName?: string; lastName?: string };

/** Wait between requests to keep campaign throughput below the provider limit. */
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Return only contacts whose provider record explicitly permits marketing. */
export async function listSubscribedContacts(): Promise<Contact[]> {
  if (!LIST_API_KEY)
    throw new Error("RESEND_CONTACTS_API_KEY / RESEND_API_KEY is not set");

  const res = await fetch("https://api.resend.com/contacts", {
    headers: { Authorization: `Bearer ${LIST_API_KEY}` },
    cache: "no-store",
  });
  if (!res.ok) {
    throw new Error(
      `Resend contacts list failed: ${res.status} ${await res.text().catch(() => "")}`,
    );
  }

  const body = (await res.json()) as {
    data?: {
      email?: string;
      first_name?: string;
      firstName?: string;
      last_name?: string;
      lastName?: string;
      unsubscribed?: boolean;
    }[];
  };

  return (
    (body.data ?? [])
      // Treat anything not explicitly false as unsubscribed. If the field is ever
      // missing or renamed, the safe failure is sending to nobody, not everybody.
      .filter((c) => c.unsubscribed === false && typeof c.email === "string")
      .map((c) => ({
        email: c.email as string,
        firstName: c.first_name ?? c.firstName,
        lastName: c.last_name ?? c.lastName,
      }))
  );
}

export type CampaignReport = {
  attempted: number;
  sent: number;
  failed: { email: string; reason: string }[];
  /** Present when the recipient list was previewed without sending. */
  dryRun?: true;
  recipients?: string[];
};

/** Preview recipients and validate locally, or send with consent filtering and pacing. */
export async function sendCampaign(input: {
  subject: string;
  html?: string;
  text?: string;
  dryRun?: boolean;
}): Promise<CampaignReport> {
  const contacts = await listSubscribedContacts();

  if (input.dryRun) {
    // Validate the template against a throwaway address so a missing
    // unsubscribe link or postal address surfaces before the real run.
    const template = prepareMarketingEmail({
      to: "preview@example.invalid",
      subject: input.subject,
      html: input.html,
      text: input.text,
    });
    const templateProblem = template.ok ? null : template.reason;

    return {
      attempted: contacts.length,
      sent: 0,
      failed: templateProblem
        ? [{ email: "(template)", reason: templateProblem }]
        : [],
      dryRun: true,
      recipients: contacts.map((c) => c.email),
    };
  }

  const failed: CampaignReport["failed"] = [];
  let sent = 0;

  for (const [i, contact] of contacts.entries()) {
    const result = await sendMarketingEmail({
      to: contact.email,
      subject: input.subject,
      html: input.html,
      text: input.text,
    });
    if (result.ok) sent++;
    else failed.push({ email: contact.email, reason: result.reason });

    // Pace, but don't wait after the final send.
    if (i < contacts.length - 1) await sleep(SEND_INTERVAL_MS);
  }

  return { attempted: contacts.length, sent, failed };
}
