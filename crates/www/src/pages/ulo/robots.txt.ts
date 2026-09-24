/** Product crawl rules with the canonical sitemap. */
export function GET() {
  return new Response(
    "User-agent: *\nAllow: /\nDisallow: /api/\n\nSitemap: https://ulo.sh/sitemap.xml\n",
    { headers: { "Content-Type": "text/plain" } },
  );
}
