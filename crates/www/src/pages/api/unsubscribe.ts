import type { APIContext } from "astro";
import { verifyUnsubToken, unsubscribeEmail } from "@/shared/email/unsubscribe";

export const prerender = false;

// RFC 8058 one-click endpoint. Mail clients (Gmail, Apple Mail) POST here when a
// recipient taps the native "Unsubscribe" button; the signed token rides in the
// URL that was placed in the List-Unsubscribe header. Per the spec this must
// return 200/202 with no body, and it must be idempotent — so we treat an
// invalid token or a downstream failure as a no-op and still return 200 rather
// than leaking whether the address was known.
export async function POST({ url }: APIContext) {
  const email = verifyUnsubToken(url.searchParams.get("token"));
  if (email) {
    const result = await unsubscribeEmail(email);
    if (!result.ok)
      console.error(
        "One-click unsubscribe failed:",
        result.reason,
        "for a token holder",
      );
  }
  return new Response(null, { status: 200 });
}

// A person opening the header URL in a browser lands here — send them to the
// branded page (which confirms before acting, so a link scanner's GET is safe).
export function GET({ url }: APIContext) {
  const token = url.searchParams.get("token") ?? "";
  return Response.redirect(
    new URL(`/unsubscribe?token=${encodeURIComponent(token)}`, url),
    307,
  );
}

/** Other methods get an empty 405, as before. */
export const ALL = () => new Response(null, { status: 405 });
