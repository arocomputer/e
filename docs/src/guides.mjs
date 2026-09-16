import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, posix, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/**
 * The guides as a site. `docs/guides/` is written for GitHub and `e docs`
 * first (docs/README.md is the contract), so this module adapts it here and
 * nowhere else: the sidebar comes from the group READMEs' front matter, a
 * relative link to a guide becomes that page's route, a link out of the folder
 * becomes GitHub, and GitHub's alert syntax becomes a Starlight aside.
 */

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const GUIDES = join(REPO, "docs/guides");
const GITHUB = "https://github.com/intuitums/e";
/** The guide the site serves at its root. */
const HOME = "getting-started";

/**
 * A file's three-key front matter and the body after it. The format is
 * deliberately not YAML (a description may hold a colon), so it is read the
 * way build.rs reads it: one `key: value` per line.
 */
function parse(path) {
  const [open, ...lines] = readFileSync(path, "utf8").split("\n");
  if (open.trimEnd() !== "---") throw new Error(`${path} has no front matter`);
  const fields = {};
  for (const [index, line] of lines.entries()) {
    if (line.trimEnd() === "---") {
      return { fields, body: lines.slice(index + 1).join("\n") };
    }
    const at = line.indexOf(":");
    if (at > 0) fields[line.slice(0, at).trim()] = line.slice(at + 1).trim();
  }
  throw new Error(`${path} has unclosed front matter`);
}

const byOrder = (a, b) => a.order - b.order || a.name.localeCompare(b.name);

/** A guide's page slug: its file stem, with getting-started at the root. */
export function slugFor(stem) {
  return stem === HOME ? "index" : stem;
}

/** Groups in order, each with its guides in order and parsed. */
function groups() {
  return readdirSync(GUIDES, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => {
      const dir = join(GUIDES, entry.name);
      const guides = readdirSync(dir)
        .filter((file) => file.endsWith(".md") && file !== "README.md")
        .map((file) => {
          const path = join(dir, file);
          const { fields, body } = parse(path);
          return { name: file.slice(0, -3), order: Number(fields.order), path, fields, body };
        })
        .sort(byOrder);
      const { fields } = parse(join(dir, "README.md"));
      return { name: entry.name, order: Number(fields.order), label: fields.title, guides };
    })
    .sort(byOrder);
}

/** Starlight's sidebar: one group per folder, its guides by slug. */
export function sidebar() {
  return groups().map((group) => ({
    label: group.label,
    items: group.guides.map((guide) => slugFor(guide.name)),
  }));
}

/**
 * The docs collection's loader: every guide, rendered through the configured
 * Markdown pipeline with its own path, so the plugins below know which guide
 * they are in.
 */
export function guidesLoader() {
  return {
    name: "e-guides",
    async load({ store, parseData, renderMarkdown, generateDigest, watcher, logger }) {
      const sync = async () => {
        store.clear();
        for (const group of groups()) {
          for (const guide of group.guides) {
            const id = slugFor(guide.name);
            const filePath = relative(join(REPO, "docs"), guide.path);
            const data = await parseData({
              id,
              filePath,
              data: { title: guide.fields.title, description: guide.fields.description },
            });
            store.set({
              id,
              data,
              body: guide.body,
              filePath,
              digest: generateDigest(guide.body + JSON.stringify(guide.fields)),
              rendered: await renderMarkdown(guide.body, { fileURL: pathToFileURL(guide.path) }),
            });
          }
        }
      };
      await sync();
      watcher?.add(GUIDES);
      watcher?.on("change", async (changed) => {
        if (!changed.startsWith(GUIDES)) return;
        logger.info(`reloading guides: ${relative(GUIDES, changed)}`);
        await sync();
      });
    },
  };
}

/**
 * Where a relative link from a guide lands: another guide's route, or the
 * file or folder on GitHub. Absolute URLs and bare fragments stay as written.
 */
export function linkTarget(url, guidePath, base) {
  if (/^([a-z]+:|#|\/)/i.test(url)) return url;
  const [path, fragment] = url.split("#");
  const hash = fragment === undefined ? "" : `#${fragment}`;
  const target = posix.normalize(
    posix.join(posix.dirname(relative(REPO, guidePath)), path),
  );
  const inGuides = /^docs\/guides\/[^/]+\/([^/]+)\.md$/.exec(target);
  if (inGuides && inGuides[1] !== "README") {
    const slug = slugFor(inGuides[1]);
    return `${base}/${slug === "index" ? "" : `${slug}/`}${hash}`;
  }
  const kind = path.endsWith("/") ? "tree" : "blob";
  return `${GITHUB}/${kind}/main/${target}${hash}`;
}

/** GitHub alert labels and the Starlight aside each one renders as. */
const ALERTS = {
  NOTE: ["note"],
  TIP: ["tip"],
  IMPORTANT: ["note", "Important"],
  WARNING: ["caution", "Warning"],
  CAUTION: ["danger", "Caution"],
};

/**
 * Sätteri mdast plugins for the guides: drop the `# Title` heading (the page
 * renders front matter's title), rewrite links, and turn alerts into the
 * container directives Starlight's aside plugin renders. Guides are the only
 * Markdown on this site, so every file is a guide.
 */
export function guidePlugins(base) {
  return ({ fileURL }) => {
    if (!fileURL) return;
    const path = fileURLToPath(fileURL);
    if (!path.startsWith(GUIDES)) return;
    return {
      name: "e-guides",
      heading(node, ctx) {
        if (node.depth === 1) ctx.removeNode(node);
      },
      link(node, ctx) {
        const url = linkTarget(node.url, path, base);
        if (url !== node.url) ctx.setProperty(node, "url", url);
      },
      blockquote(node) {
        const [first, ...rest] = node.children;
        const marker = first?.type === "paragraph" && first.children[0];
        const match =
          marker?.type === "text" && /^\[!([A-Z]+)\]\s*/.exec(marker.value);
        const alert = match && ALERTS[match[1]];
        if (!alert) return;
        const [variant, title] = alert;
        const text = marker.value.slice(match[0].length);
        const lead = [
          ...(text ? [{ ...marker, value: text }] : []),
          ...first.children.slice(1),
        ];
        const children = [
          ...(title
            ? [
                {
                  type: "paragraph",
                  data: { directiveLabel: true },
                  children: [{ type: "text", value: title }],
                },
              ]
            : []),
          ...(lead.length ? [{ type: "paragraph", children: lead }] : []),
          ...rest,
        ];
        return {
          type: "containerDirective",
          name: variant,
          attributes: {},
          children,
        };
      },
    };
  };
}
