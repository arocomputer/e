/** Record explicit marketing consent after the contact notification is accepted. */
const RESEND_API_KEY =
  process.env.RESEND_CONTACTS_API_KEY || process.env.RESEND_API_KEY;

/** Normalize an address for the provider's shared contact list. */
const normalize = (email: string) => email.trim().toLowerCase();

export type SubscribeResult =
  { ok: true } | { ok: false; reason: "not_configured" | "failed" };

/** Record consent; callers handle network failures independently of notification delivery. */
export async function subscribeContact(input: {
  email: string;
  firstName?: string;
  lastName?: string;
}): Promise<SubscribeResult> {
  if (!RESEND_API_KEY) return { ok: false, reason: "not_configured" };

  const res = await fetch("https://api.resend.com/contacts", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${RESEND_API_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      email: normalize(input.email),
      firstName: input.firstName,
      lastName: input.lastName,
      unsubscribed: false,
    }),
  });

  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    console.error("Resend subscribe error:", res.status, detail);
    return { ok: false, reason: "failed" };
  }
  return { ok: true };
}
