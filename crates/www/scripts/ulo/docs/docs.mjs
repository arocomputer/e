import { existsSync } from "node:fs";
import { lstat, mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Render this checkout's docs/guides into the pages, navigation, and search index.
 * ULO_DOCS_PATH can select another checkout; no build fetches a second repository.
 * The front matter contract lives in the repository's docs/README.md.
 */

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const OUT = join(ROOT, "src/generated/docs.json");
const REPO = "arocomputer/ulo";
const MAX_GROUPS = 20;
const MAX_GUIDES = 100;
const MAX_GUIDE_BYTES = 512 * 1024;

/** The guide the website renders at `/ulo/docs`; ulo's `docs/README.md` names it. */
export const HOME_SLUG = "getting-started";

/** Accept ordinary Git refs and commit hashes, never URL syntax or traversal. */
export function docsRef(value = "main") {
  if (
    !/^[A-Za-z0-9][A-Za-z0-9._/-]*$/.test(value) ||
    value.includes("..") ||
    value.includes("//") ||
    value.endsWith("/") ||
    value.endsWith(".lock")
  ) {
    throw new Error(`invalid ULO_DOCS_REF: ${value}`);
  }
  return value;
}

const REF = docsRef(process.env.ULO_DOCS_REF);
const BLOB_REF = REF.split("/").map(encodeURIComponent).join("/");
const BLOB = `https://github.com/${REPO}/blob/${BLOB_REF}`;

/**
 * Where the guides sit inside a checkout: `docs/guides/`, or `docs/` itself in
 * refs from before ulo gave the folder to its docs site. The prefix is the
 * guides' repository path, which link rewriting resolves against.
 */
export function guidesIn(checkout) {
  const nested = resolve(checkout, "docs", "guides");
  return existsSync(nested)
    ? { dir: nested, prefix: "docs/guides" }
    : { dir: resolve(checkout, "docs"), prefix: "docs" };
}

/** Resolve the guides in this repository, or an explicitly selected checkout. */
export async function docsDir() {
  const local = process.env.ULO_DOCS_PATH || resolve(ROOT, "../..");
  const docs = resolve(local, "docs", "guides");
  if (!existsSync(docs))
    throw new Error(`ULO_DOCS_PATH has no docs/guides/: ${docs}`);
  return guidesIn(local);
}

/** Parse the deliberately small, closed front matter format used by ulo. */
export function frontMatter(text, source = "guide") {
  const lines = text.split("\n");
  if (lines[0]?.trimEnd() !== "---") return { fields: {}, body: text };
  const fields = {};
  let end = -1;
  for (let index = 1; index < lines.length; index += 1) {
    if (lines[index].trimEnd() === "---") {
      end = index;
      break;
    }
    if (lines[index].trim() === "") continue;
    const at = lines[index].indexOf(":");
    if (at < 1) throw new Error(`${source} has malformed front matter`);
    const key = lines[index].slice(0, at).trim();
    if (!["title", "description", "order"].includes(key)) {
      throw new Error(`${source} has unknown front matter \`${key}\``);
    }
    if (Object.hasOwn(fields, key)) {
      throw new Error(`${source} repeats front matter \`${key}\``);
    }
    fields[key] = lines[index].slice(at + 1).trim();
  }
  if (end < 0) throw new Error(`${source} has unclosed front matter`);
  return { fields, body: lines.slice(end + 1).join("\n") };
}

/** Validate metadata before it becomes a route, heading, or sort key. */
function metadata(text, source) {
  const { fields, body } = frontMatter(text, source);
  for (const key of ["title", "description", "order"]) {
    if (!fields[key])
      throw new Error(`${source} needs front matter \`${key}\``);
  }
  const order = Number(fields.order);
  if (!Number.isSafeInteger(order) || order < 0) {
    throw new Error(`${source} has invalid front matter \`order\``);
  }
  if (fields.title.length > 100 || fields.description.length > 300) {
    throw new Error(`${source} has oversized front matter`);
  }
  return { ...fields, order, body };
}

/** Read one regular Markdown file with a per-guide size ceiling. */
async function guideFile(path, source) {
  const details = await lstat(path);
  if (!details.isFile()) throw new Error(`${source} is not a regular file`);
  if (details.size > MAX_GUIDE_BYTES) {
    throw new Error(`${source} exceeds ${MAX_GUIDE_BYTES} bytes`);
  }
  return readFile(path, "utf8");
}

/**
 * Heading text to anchor id. The rule follows GitHub's — lowercase, punctuation
 * dropped, spaces to dashes — so an anchor that works in the repository
 * (`extensions.md#results-by-method`) also works on the site.
 */
export function slug(text) {
  return text
    .toLowerCase()
    .trim()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s+/g, "-");
}

/**
 * Split a guide into its lead, one section per `##` heading, and one subsection
 * per `###` inside it. The page component renders those headings itself so it
 * can build "on this page" from them, which is why none are left in the body.
 */
export function sections(body) {
  const withoutTitle = body.replace(/^\s*#\s+.*\n/, "");
  const parts = withoutTitle.split(/^## /m);
  const lead = parts.shift().trim();
  const heading = (block) => {
    const [title, ...rest] = block.split("\n");
    const trimmed = title.trim();
    return {
      id: slug(trimmed),
      title: trimmed,
      body: rest.join("\n").trim(),
    };
  };
  return {
    lead,
    sections: parts.map((part) => {
      const [first, ...rest] = part.split(/^### /m);
      return { ...heading(first), subsections: rest.map(heading) };
    }),
  };
}

/**
 * Point a guide's links at their destinations on the site: another guide
 * becomes its route, and anything outside `docs/` becomes the repository, since
 * only this folder is published.
 */
export function rewriteLinks(body, sourcePath, routes) {
  return body.replace(/\]\(([^)\s]+)\)/g, (whole, target) => {
    if (/^(https?:|mailto:|#)/.test(target)) return whole;
    const [file, fragment] = target.split("#");
    if (!file) return whole;
    const resolved = posix.normalize(
      posix.join(posix.dirname(sourcePath), file),
    );
    const route = routes.get(resolved);
    if (route) return `](${route}${fragment ? `#${fragment}` : ""})`;
    return `](${BLOB}/${resolved}${fragment ? `#${fragment}` : ""})`;
  });
}

/**
 * GitHub's alert syntax — `> [!NOTE]` and friends — becomes the site's callout.
 * GitHub renders those lines itself and the binary prints them unchanged, so a
 * guide gains a callout by writing markdown.
 */
export function callouts(body) {
  const kinds = {
    NOTE: "note",
    TIP: "tip",
    IMPORTANT: "important",
    WARNING: "warning",
    CAUTION: "caution",
  };
  const lines = body.split("\n");
  const out = [];
  for (let at = 0; at < lines.length; at += 1) {
    const marker = /^> ?\[!([A-Z]+)\]\s*$/.exec(lines[at]);
    const kind = marker && kinds[marker[1]];
    if (!kind) {
      out.push(lines[at]);
      continue;
    }
    const quoted = [];
    while (at + 1 < lines.length && /^> ?/.test(lines[at + 1])) {
      at += 1;
      quoted.push(lines[at].replace(/^> ?/, ""));
    }
    out.push(`<Callout kind="${kind}">`, "", ...quoted, "", "</Callout>");
  }
  return out.join("\n");
}

/**
 * The generated index: groups in order, then their guides. `prefix` is the
 * guides' path in the repository, so a link out of the folder reaches GitHub.
 */
export async function build(docs, prefix = "docs") {
  const entries = await readdir(docs, { withFileTypes: true });
  const groupEntries = entries.filter((item) => item.isDirectory());
  if (!groupEntries.length || groupEntries.length > MAX_GROUPS) {
    // A ref that predates the grouped layout keeps its guides loose in docs/.
    // This is the first line of a build log, so name the cause and the way out
    // instead of reporting a count.
    const loose = entries.filter(
      (entry) =>
        entry.isFile() &&
        /\.mdx?$/.test(entry.name) &&
        entry.name !== "README.md",
    );
    const hint = loose.length
      ? `: ${loose.length} loose guides, so ${REF} predates the folder-per-group layout ` +
        "(each folder needs a README naming and ordering it — see docs/README.md in arocomputer/ulo). " +
        "Merge that change, or point ULO_DOCS_REF at a ref that has it"
      : "";
    throw new Error(`docs/ has ${groupEntries.length} groups${hint}`);
  }
  const groups = [];
  for (const entry of groupEntries) {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(entry.name)) {
      throw new Error(`invalid documentation group: ${entry.name}`);
    }
    const source = `${entry.name}/README.md`;
    const readme = join(docs, entry.name, "README.md");
    if (!existsSync(readme)) {
      throw new Error(`${entry.name}/ has no README.md naming the group`);
    }
    const fields = metadata(await guideFile(readme, source), source);
    groups.push({
      slug: entry.name,
      title: fields.title,
      description: fields.description,
      order: fields.order,
    });
  }
  groups.sort((a, b) => a.order - b.order || a.slug.localeCompare(b.slug));

  // Guides first, so link rewriting knows every route before it runs.
  const sources = [];
  const slugs = new Set();
  for (const group of groups) {
    const entries = await readdir(join(docs, group.slug), {
      withFileTypes: true,
    });
    for (const entry of entries) {
      if (!entry.isFile() || !/\.mdx?$/.test(entry.name)) continue;
      if (entry.name === "README.md") continue;
      const path = join(docs, group.slug, entry.name);
      const source = posix.join(prefix, group.slug, entry.name);
      const fields = metadata(await guideFile(path, source), source);
      const slugName = entry.name.replace(/\.mdx?$/, "");
      if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slugName)) {
        throw new Error(`invalid documentation slug: ${source}`);
      }
      if (slugs.has(slugName)) {
        throw new Error(`duplicate documentation slug: ${slugName}`);
      }
      slugs.add(slugName);
      sources.push({
        slug: slugName,
        route: slugName === HOME_SLUG ? "/ulo/docs" : `/ulo/docs/${slugName}`,
        group: group.slug,
        order: fields.order,
        title: fields.title,
        description: fields.description,
        source,
        body: fields.body,
      });
      if (sources.length > MAX_GUIDES) {
        throw new Error(`docs/ exceeds ${MAX_GUIDES} guides`);
      }
    }
  }
  if (!sources.some((doc) => doc.slug === HOME_SLUG)) {
    throw new Error(
      `docs/ has no ${HOME_SLUG}.md, and the website's /docs page renders it ` +
        "(docs/README.md in arocomputer/ulo explains the guide). Merge that guide, " +
        "or point ULO_DOCS_PATH at a checkout that has it",
    );
  }
  const routes = new Map(sources.map((doc) => [doc.source, doc.route]));

  const docs_ = sources
    .sort((a, b) => a.order - b.order || a.slug.localeCompare(b.slug))
    .map((doc) => {
      const body = callouts(rewriteLinks(doc.body, doc.source, routes));
      const split = sections(body);
      const ids = split.sections.flatMap((section) => [
        section.id,
        ...section.subsections.map((subsection) => subsection.id),
      ]);
      if (ids.some((id, index) => !id || ids.indexOf(id) !== index)) {
        throw new Error(`${doc.source} has empty or duplicate heading anchors`);
      }
      return {
        slug: doc.slug,
        route: doc.route,
        group: doc.group,
        order: doc.order,
        title: doc.title,
        description: doc.description,
        source: doc.source,
        lead: split.lead || doc.description,
        sections: split.sections,
      };
    });

  // Every anchor a guide points at must exist, on its own page or another:
  // GitHub resolves a fragment by luck, the site either has the heading or
  // ships a dead link.
  const anchors = new Map(
    docs_.map((doc) => [
      doc.route,
      new Set(
        doc.sections.flatMap((section) => [
          section.id,
          ...section.subsections.map((sub) => sub.id),
        ]),
      ),
    ]),
  );
  for (const doc of docs_) {
    const text =
      doc.sections.map((section) => section.body).join("\n") + doc.lead;
    for (const [, target, fragment] of text.matchAll(
      /\]\(([^)#]*)#([a-z0-9-]+)\)/g,
    )) {
      const page = target === "" ? doc.route : target;
      if (!anchors.has(page)) continue;
      if (!anchors.get(page).has(fragment)) {
        throw new Error(
          `${doc.source} links to ${target || "this page"}#${fragment}, which no heading has`,
        );
      }
    }
  }

  return { ref: REF, groups, docs: docs_ };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const { dir, prefix } = await docsDir();
  const index = await build(dir, prefix);
  await mkdir(dirname(OUT), { recursive: true });
  await writeFile(OUT, JSON.stringify(index, null, 2) + "\n");
  console.log(
    `ulo docs: ${index.docs.length} guides in ${index.groups.length} groups (${index.ref})`,
  );
}
