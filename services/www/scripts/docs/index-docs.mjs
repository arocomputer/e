import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import * as pagefind from "pagefind";

/**
 * Index only built ulo articles, using their real routes and heading anchors.
 * Runs after `astro build`: reads the prerendered pages under
 * `dist/client/` and writes the bundle the docs search loads from
 * `/pagefind/`.
 */
async function indexDocs() {
  const base = "dist/client";
  const nested = await readdir(join(base, "docs"));
  const files = [
    "docs.html",
    ...nested
      .filter((name) => name.endsWith(".html"))
      .map((name) => `docs/${name}`),
  ];
  const { index, errors } = await pagefind.createIndex({
    rootSelector: "[data-pagefind-body]",
    forceLanguage: "en",
  });
  if (!index || errors.length)
    throw new Error(errors.join("\n") || "Could not create docs index");
  for (const file of files) {
    const content = await readFile(join(base, file), "utf8");
    if (!content.includes("data-pagefind-body"))
      throw new Error(`Missing search body: ${file}`);
    const result = await index.addHTMLFile({
      url: `/${file.replace(/\.html$/, "")}`,
      content,
    });
    if (result.errors.length) throw new Error(result.errors.join("\n"));
  }
  const result = await index.writeFiles({
    outputPath: join(base, "pagefind"),
  });
  if (result.errors.length) throw new Error(result.errors.join("\n"));
  console.log(`Indexed ${files.length} ulo documentation pages.`);
}

try {
  await indexDocs();
} finally {
  await pagefind.close();
}
