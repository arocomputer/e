import assert from "node:assert/strict";
import test from "node:test";
import worker from "./worker.mjs";

/** Legacy host redirects preserve product paths and queries. */
test("ulo bookmarks reach e's canonical site", async () => {
  for (const [source, destination] of [
    ["https://ulo.sh/", "https://aro.computer/e"],
    ["https://www.ulo.sh/docs?from=old", "https://aro.computer/e/docs?from=old"],
    ["https://ulo.sh/ulo/docs", "https://aro.computer/e/docs"],
    ["https://ulo.sh/e/legal", "https://aro.computer/e/legal"],
    ["https://ulo.sh/api/unsubscribe?token=abc", "https://aro.computer/api/unsubscribe?token=abc"],
  ]) {
    const response = await worker.fetch(new Request(source));
    assert.equal(response.status, 308);
    assert.equal(response.headers.get("location"), destination);
  }
});
