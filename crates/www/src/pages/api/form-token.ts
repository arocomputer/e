import { isConfigured, mintFormToken } from "@/shared/contact/form-token";

// Mints the timing token the contact form submits back.
export const prerender = false;

/** Issue an uncached token outside the contact request budget. */
export function GET() {
  return Response.json(
    { token: isConfigured ? mintFormToken() : null },
    { headers: { "Cache-Control": "no-store" } },
  );
}
