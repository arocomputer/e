import png from "@/sites/ulo/assets/social.png?inline";

/**
 * ulo's share card, rendered ahead of time by `scripts/ulo/social/render.mjs` (run
 * `npm run social` after changing it). Vite inlines the PNG as a base64 data
 * URL. Served on demand because a prerendered `/ulo/social` asset has no
 * extension, so Cloudflare would send it without a content type.
 */
export const prerender = false;

export function GET() {
  const bytes = Uint8Array.from(atob(png.slice(png.indexOf(",") + 1)), (c) =>
    c.charCodeAt(0),
  );
  return new Response(bytes, {
    headers: {
      "Content-Type": "image/png",
      "Cache-Control": "public, max-age=0, must-revalidate",
    },
  });
}
