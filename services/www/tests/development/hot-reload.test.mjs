import assert from "node:assert/strict";
import { readFile, utimes } from "node:fs/promises";
import { setTimeout } from "node:timers/promises";
import test from "node:test";

const origin = process.env.DEV_TEST_ORIGIN || "http://localhost:4321";

/** Exercise overlapping Worker invalidations, including server errors hidden by client hydration. */
test("concurrent Worker edits keep the renderer and React on the same module generation", async () => {
  const log = process.env.DEV_TEST_LOG;
  assert.ok(
    log,
    "DEV_TEST_LOG must name the running Astro server's output file",
  );
  const response = await fetch(origin);
  assert.equal(response.status, 200);
  await response.text();
  const offset = (await readFile(log, "utf8")).length;
  for (let round = 0; round < 3; round++) {
    const now = new Date();
    await Promise.all(
      ["worker.ts", "data/docs-aliases.ts"].map((path) =>
        utimes(new URL(`../../src/${path}`, import.meta.url), now, now),
      ),
    );
    // Request throughout the reload, not only after the server has recovered.
    for (let attempt = 0; attempt < 10; attempt++) {
      const page = await fetch(origin);
      assert.equal(page.status, 200);
      assert.match(await page.text(), /The coding agent you can put anywhere/);
      await setTimeout(50);
    }
  }
  const output = (await readFile(log, "utf8")).slice(offset);
  assert.match(
    output,
    /reload/,
    "the test must actually invalidate the Worker",
  );
  assert.doesNotMatch(output, /Invalid hook call|\[ERROR\]/);
});
