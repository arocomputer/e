import type { Element, ElementContent, Root as HastRoot } from "hast";
import type { Parent, Root as MdastRoot, RootContent } from "mdast";
import { toText } from "hast-util-to-text";
import rehypeHighlight from "rehype-highlight";
import rehypeStringify from "rehype-stringify";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import remarkRehype from "remark-rehype";
import { unified } from "unified";
import { visit } from "unist-util-visit";

/**
 * Render one guide's markdown to the article HTML the docs pages embed, at
 * build time. GitHub Flavored Markdown brings the tables, task lists, and
 * autolinks the guides are written in; fenced blocks gain highlight classes
 * here, so the pages ship no highlighter and the code theme lives in
 * `docs.css` alone. Raw HTML in a guide is dropped, never passed through: the
 * only tags the import emits are its `<Callout kind="…">` pairs, which become
 * the callout markup below. Copy buttons are wired by the script in `DocArticle.astro`.
 */
export function renderMarkdown(source: string): string {
  return String(processor.processSync(source));
}

const CALLOUT_LABELS: Record<string, string> = {
  note: "Note",
  tip: "Tip",
  important: "Important",
  warning: "Warning",
  caution: "Caution",
};

/** Fenced blocks are labelled by their info string, not GitHub's class. */
const CODE_LABELS: Record<string, string> = {
  bash: "Shell",
  sh: "Shell",
  shell: "Shell",
  json: "JSON",
  jsonl: "JSONL",
  toml: "TOML",
  js: "JavaScript",
  mjs: "JavaScript",
  ts: "TypeScript",
  rust: "Rust",
  md: "Markdown",
  markdown: "Markdown",
  yaml: "YAML",
};

const OPEN_CALLOUT = /^<Callout kind="([a-z]+)">$/;

/** Wrap the blocks between an import-emitted `<Callout>` pair in the callout's markup. */
function remarkCallouts() {
  return (tree: MdastRoot) => {
    visit(tree, (node) => {
      if (!("children" in node)) return;
      const children = (node as Parent).children;
      for (let at = 0; at < children.length; at += 1) {
        const open = children[at];
        const kind =
          open.type === "html" ? OPEN_CALLOUT.exec(open.value)?.[1] : undefined;
        if (!kind) continue;
        const end = children.findIndex(
          (child, index) =>
            index > at && child.type === "html" && child.value === "</Callout>",
        );
        if (end < 0) continue;
        const callout = {
          type: "callout",
          data: {
            hName: "aside",
            hProperties: {
              className: ["ulo-doc-callout"],
              dataKind: kind,
            },
          },
          children: [
            {
              type: "paragraph",
              data: {
                hProperties: {
                  className: ["ulo-doc-callout-label"],
                },
              },
              children: [
                {
                  type: "text",
                  value: CALLOUT_LABELS[kind] ?? "Note",
                },
              ],
            },
            {
              type: "calloutBody",
              data: {
                hName: "div",
                hProperties: {
                  className: ["ulo-doc-callout-body"],
                },
              },
              children: children.slice(at + 1, end),
            },
          ],
        } as unknown as RootContent;
        children.splice(at, end - at + 1, callout);
      }
    });
  };
}

const element = (
  tagName: string,
  properties: Element["properties"],
  children: ElementContent[] = [],
): Element => ({ type: "element", tagName, properties, children });

function copyButton(label: string, children: ElementContent[]): Element {
  const icon = element("svg", { viewBox: "0 0 24 24", ariaHidden: "true" }, [
    element("rect", { x: "8", y: "8", width: "12", height: "12", rx: "2" }),
    element("path", {
      d: "M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3",
    }),
  ]);
  return element(
    "button",
    {
      className: ["ulo-copy"],
      type: "button",
      ariaLabel: `Copy ${label}`,
      title: `Copy ${label}`,
      dataCopied: "false",
    },
    [...children, icon],
  );
}

/**
 * A fenced block as a code block. A labelled block is one a reader runs or
 * reuses, so it offers copying: a single line copies from the code itself, a
 * longer one from the header icon alone. A fence with no language is an
 * example: no caption and nothing to copy.
 */
function rehypeCodeBlocks() {
  return (tree: HastRoot) => {
    visit(tree, "element", (node, index, parent) => {
      const code = node.children[0];
      if (
        node.tagName !== "pre" ||
        !parent ||
        index === undefined ||
        code?.type !== "element" ||
        code.tagName !== "code"
      )
        return;
      const classes = (code.properties.className ?? []) as string[];
      const language =
        classes
          .map((name) => /^language-([\w-]+)$/.exec(String(name))?.[1])
          .find(Boolean) ?? "";
      const label = CODE_LABELS[language] ?? (language || undefined);
      const multiline = /[\r\n]/.test(
        toText(code, { whitespace: "pre" }).trimEnd(),
      );
      const text = element(
        "code",
        { className: ["ulo-doc-code-text"] },
        code.children,
      );
      const block = element("div", { className: ["ulo-doc-code"] });
      if (label !== undefined) {
        block.children.push(
          element(
            "div",
            {
              className: ["ulo-doc-code-bar"],
              dataPagefindIgnore: "true",
            },
            [
              element("span", {}, [{ type: "text", value: label }]),
              ...(multiline ? [copyButton(label, [])] : []),
            ],
          ),
        );
      }
      block.children.push(
        element("div", { className: ["ulo-doc-code-body"] }, [
          label !== undefined && !multiline ? copyButton(label, [text]) : text,
        ]),
      );
      if (label !== undefined) {
        block.children.push(
          element("span", {
            className: ["ulo-visually-hidden"],
            role: "status",
            dataPagefindIgnore: "true",
          }),
        );
      }
      parent.children[index] = block;
      return "skip";
    });
  };
}

/**
 * Tables carry no whitespace text between their rows and cells, as the MDX
 * pipeline the site used to render them with produced.
 */
function rehypeTables() {
  return (tree: HastRoot) => {
    visit(tree, "element", (node) => {
      if (["table", "thead", "tbody", "tfoot", "tr"].includes(node.tagName))
        node.children = node.children.filter(
          (child) => child.type !== "text" || child.value.trim() !== "",
        );
    });
  };
}

const processor = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .use(remarkCallouts)
  .use(remarkRehype)
  .use(rehypeHighlight)
  .use(rehypeCodeBlocks)
  .use(rehypeTables)
  .use(rehypeStringify, {
    characterReferences: { useNamedReferences: true },
  });
