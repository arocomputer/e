import index from "@/generated/docs.json";

/**
 * The guides, as `scripts/ulo/docs/docs.mjs` produced them from arocomputer/ulo.
 *
 * `ulo`'s `docs/` is the only copy: this module never invents content, it only
 * shapes the generated index for the pages, the sidebar, the search entries,
 * and llms.txt. Regenerate with `npm run docs`, which the build and dev scripts
 * already run.
 */

export type DocSection = {
  id: string;
  title: string;
  body: string;
  subsections: { id: string; title: string; body: string }[];
};

export type Doc = {
  slug: string;
  route: string;
  group: string;
  order: number;
  title: string;
  description: string;
  source: string;
  lead: string;
  sections: DocSection[];
};

export type Group = {
  slug: string;
  title: string;
  description: string;
  order: number;
};

export type DocNavGroup = {
  href: string;
  label: string;
  links: [string, string][];
};

export const groups: Group[] = index.groups;
export const docs: Doc[] = index.docs as Doc[];

/** The docs root. `scripts/ulo/docs/docs.mjs` gives this route to the home guide. */
export const HOME_ROUTE = "/ulo/docs";

/** The guide the docs root renders; the import fails without one. */
export function home(): Doc {
  const entry = docs.find((item) => item.route === HOME_ROUTE);
  if (!entry) throw new Error(`the docs index has no ${HOME_ROUTE} guide`);
  return entry;
}

/** Guides of one group, in their authored order. */
export function groupDocs(slug: string): Doc[] {
  return docs
    .filter((entry) => entry.group === slug)
    .sort((a, b) => a.order - b.order || a.slug.localeCompare(b.slug));
}

/** One guide by its route segment. */
export function doc(slug: string): Doc | undefined {
  return docs.find((entry) => entry.slug === slug);
}

/** The order the sidebar reads in: group order, then the guide's own. */
export function readingOrder(): Doc[] {
  return [...groups]
    .sort((a, b) => a.order - b.order)
    .flatMap((group) => groupDocs(group.slug));
}

/** The guides either side of one route, for the footer links. */
export function neighbours(route: string): { previous?: Doc; next?: Doc } {
  const order = readingOrder();
  const at = order.findIndex((entry) => entry.route === route);
  if (at < 0) return {};
  return { previous: order[at - 1], next: order[at + 1] };
}

/** Convert an internal preview route to its canonical product-host path. */
export function publicDocPath(route: string): string {
  if (!route.startsWith("/ulo/docs")) {
    throw new Error(`not an ulo documentation route: ${route}`);
  }
  return route.slice("/ulo".length);
}

/** Sidebar shape: each linked group and its guides. */
export function sidebar(): DocNavGroup[] {
  return [...groups]
    .sort((a, b) => a.order - b.order)
    .map((group) => ({
      href: `/ulo/docs/${group.slug}`,
      label: group.title,
      links: groupDocs(group.slug).map(
        (entry) => [entry.route, entry.title] as [string, string],
      ),
    }));
}
