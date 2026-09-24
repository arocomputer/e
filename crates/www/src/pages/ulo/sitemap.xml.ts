import { docs, groups, publicDocPath } from "@/sites/ulo/components/docs/data";

/** Index ulo's stable pages and every generated guide on the canonical domain. */
export function GET() {
  // The home guide's route is `/docs`, which the fixed paths already carry.
  const paths = new Set([
    "/",
    "/changelog",
    "/docs",
    ...groups.map((group) => `/docs/${group.slug}`),
    ...docs.map((entry) => publicDocPath(entry.route)),
    "/legal",
    "/legal/license",
    "/legal/privacy",
    "/legal/terms",
    "/legal/report-vulnerability",
    "/legal/safety",
    "/contact",
  ]);
  const urls = [...paths]
    .map((path) => `<url><loc>https://ulo.sh${path}</loc></url>`)
    .join("");
  return new Response(
    `<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${urls}</urlset>`,
    { headers: { "Content-Type": "application/xml" } },
  );
}
