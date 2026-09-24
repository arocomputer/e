import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { renderMarkdown } from "../../src/sites/ulo/components/docs/markdown.ts";
import {
  build,
  callouts,
  docsRef,
  frontMatter,
  guidesIn,
  HOME_SLUG,
  slug,
} from "../../scripts/ulo/docs/docs.mjs";

/**
 * The import reads ulo's `docs/` and turns it into this site's pages. These pin
 * the contract the repository documents in its own `docs/README.md`: the folder
 * is the group, the file stem is the route, front matter is the metadata, and a
 * link to another guide becomes that guide's page.
 */

const lines = (...rows) => rows.join("\n") + "\n";

/** A small docs tree with the shapes that matter, built in a temp directory. */
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "docs-fixture-"));
  await mkdir(join(root, "start"), { recursive: true });
  await mkdir(join(root, "usage"), { recursive: true });
  await writeFile(
    join(root, "start", "README.md"),
    lines("---", "title: Start", "description: begin here", "order: 1", "---"),
  );
  await writeFile(
    join(root, "usage", "README.md"),
    lines("---", "title: Usage", "description: day to day", "order: 2", "---"),
  );
  await writeFile(
    join(root, "start", "getting-started.md"),
    lines(
      "---",
      "title: Getting started",
      "description: install ulo",
      "order: 1",
      "---",
      "",
      "Install it.",
    ),
  );
  await writeFile(
    join(root, "start", "models.md"),
    lines(
      "---",
      "title: Models",
      "description: models.json",
      "order: 2",
      "---",
      "",
      "Models come from a file.",
      "",
      "## Dialects",
      "",
      "See [sessions](../usage/sessions.md) and [the source](../../src/lib.rs).",
      "",
      "Open [getting started](getting-started.md).",
      "",
      "### Nested heading",
      "",
      "More prose.",
    ),
  );
  await writeFile(
    join(root, "usage", "sessions.md"),
    lines(
      "---",
      "title: Sessions",
      "description: resume work",
      "order: 1",
      "---",
      "",
      "A session is a conversation.",
      "",
      "## Resume",
      "",
      "Back to [models](../start/models.md#dialects).",
    ),
  );
  return root;
}

test("a group's README names it and orders it", async () => {
  const index = await build(await fixture());
  assert.deepEqual(
    index.groups.map((group) => [group.slug, group.title, group.order]),
    [
      ["start", "Start", 1],
      ["usage", "Usage", 2],
    ],
  );
});

test("a guide becomes a page with its sections and subsections", async () => {
  const index = await build(await fixture());
  const models = index.docs.find((entry) => entry.slug === "models");
  assert.equal(models.route, "/ulo/docs/models");
  assert.equal(models.title, "Models");
  assert.equal(models.lead, "Models come from a file.");
  assert.deepEqual(
    models.sections.map((section) => [section.id, section.title]),
    [["dialects", "Dialects"]],
  );
  assert.deepEqual(
    models.sections[0].subsections.map((sub) => sub.id),
    ["nested-heading"],
  );
});

test("links point at routes, or at the repository when they are not pages", async () => {
  const index = await build(await fixture());
  const body = index.docs.find((entry) => entry.slug === "models").sections[0]
    .body;
  assert.match(body, /\]\(\/ulo\/docs\/sessions\)/);
  assert.match(
    body,
    /\]\(https:\/\/github\.com\/arocomputer\/ulo\/blob\/main\/src\/lib\.rs\)/,
  );
});

test("guides under docs/guides link out of the repository from their real path", async () => {
  const checkout = await mkdtemp(join(tmpdir(), "docs-checkout-"));
  const start = join(checkout, "docs", "guides", "start");
  await mkdir(start, { recursive: true });
  await writeFile(
    join(start, "README.md"),
    lines("---", "title: Start", "description: begin here", "order: 1", "---"),
  );
  await writeFile(
    join(start, "getting-started.md"),
    lines(
      "---",
      "title: Getting started",
      "description: install ulo",
      "order: 1",
      "---",
      "",
      "Read [the releases guide](../../../contributing/releases.md).",
    ),
  );
  const { dir, prefix } = guidesIn(checkout);
  assert.equal(prefix, "docs/guides");
  const index = await build(dir, prefix);
  assert.match(
    index.docs[0].lead,
    /\]\(https:\/\/github\.com\/arocomputer\/ulo\/blob\/main\/contributing\/releases\.md\)/,
  );
  await rm(checkout, { recursive: true, force: true });
});

test("the getting-started guide is the docs root", async () => {
  const index = await build(await fixture());
  const home = index.docs.find((entry) => entry.slug === HOME_SLUG);
  assert.equal(home.route, "/ulo/docs");
  const body = index.docs.find((entry) => entry.slug === "models").sections[0]
    .body;
  assert.match(body, /\]\(\/ulo\/docs\)/, "a link to it is not the root");
});

test("docs without a getting-started guide stop the import", async () => {
  const root = await fixture();
  await rm(join(root, "start", `${HOME_SLUG}.md`));
  await assert.rejects(() => build(root), /has no getting-started\.md/);
});

test("a guide without front matter stops the import", async () => {
  const root = await fixture();
  await writeFile(
    join(root, "usage", "sessions.md"),
    lines("# Sessions", "", "No front matter."),
  );
  await assert.rejects(() => build(root), /needs front matter/);
});

test("a group README without an order stops the import", async () => {
  const root = await fixture();
  await writeFile(
    join(root, "usage", "README.md"),
    lines("---", "title: Usage", "description: day to day", "---"),
  );
  await assert.rejects(() => build(root), /needs front matter `order`/);
});

test("source refs cannot inject traversal into GitHub links", () => {
  assert.equal(docsRef("feat/docs-one-source"), "feat/docs-one-source");
  for (const ref of [
    "../main",
    "main?download=1",
    "main//other",
    "main.lock",
  ]) {
    assert.throws(() => docsRef(ref), /invalid ULO_DOCS_REF/);
  }
});

test("front matter is closed, unique, and limited to its contract", () => {
  assert.throws(
    () => frontMatter(lines("---", "title: Missing end"), "bad.md"),
    /unclosed front matter/,
  );
  assert.throws(
    () =>
      frontMatter(lines("---", "title: One", "title: Two", "---"), "bad.md"),
    /repeats front matter `title`/,
  );
  assert.throws(
    () => frontMatter(lines("---", "script: nope", "---"), "bad.md"),
    /unknown front matter `script`/,
  );
});

test("duplicate routes stop the import", async () => {
  const root = await fixture();
  await writeFile(
    join(root, "usage", "models.md"),
    lines(
      "---",
      "title: Other models",
      "description: duplicate route",
      "order: 3",
      "---",
      "",
      "# Other models",
      "",
      "## Duplicate",
    ),
  );
  await assert.rejects(
    () => build(root),
    /duplicate documentation slug: models/,
  );
});

test("repository markdown cannot inject HTML", () => {
  const html = renderMarkdown(
    'Safe\n\n<script>alert(1)</script>\n\n<img src="x" onerror="alert(1)">',
  );
  assert.match(html, /Safe/);
  assert.doesNotMatch(html, /<script|<img|onerror/);
});

test("the import's callouts render as the site's callout", () => {
  assert.equal(
    renderMarkdown(
      lines('<Callout kind="warning">', "", "Keep a copy.", "", "</Callout>"),
    ),
    '<aside class="ulo-doc-callout" data-kind="warning"><p class="ulo-doc-callout-label">Warning</p><div class="ulo-doc-callout-body"><p>Keep a copy.</p></div></aside>',
  );
});

test("GitHub alerts become fixed callout components", () => {
  assert.equal(
    callouts(lines("> [!WARNING]", "> Keep a copy.", "", "Continue.")),
    lines(
      '<Callout kind="warning">',
      "",
      "Keep a copy.",
      "",
      "</Callout>",
      "",
      "Continue.",
    ),
  );
});

test("a flat docs/ says what it needs instead of naming a guide", async () => {
  const root = await mkdtemp(join(tmpdir(), "docs-flat-"));
  await writeFile(
    join(root, "models.md"),
    lines(
      "---",
      "title: Models",
      "description: models",
      "order: 1",
      "---",
      "",
      "Prose.",
    ),
  );
  await assert.rejects(() => build(root), /loose guides/);
});

test("heading slugs match the anchors the repository already uses", () => {
  assert.equal(
    slug("Asking ulo: `ui.*` and `session.*`"),
    "asking-ulo-ui-and-session",
  );
  assert.equal(slug("The side pane"), "the-side-pane");
  assert.equal(slug("Results by method"), "results-by-method");
});
