import assert from "node:assert/strict";
import test from "node:test";
import { parseRelease, publishedReleases } from "../src/data/releases.ts";
const release = {
  version: "1.2.3",
  title: "A release",
  intro: "Changes",
  groups: { "New features": ["A"], Improvements: [], Fixes: [] },
};
test("a rate-limited lookup keeps the releases it already has", async (t) => {
  t.mock.method(globalThis, "fetch", async () =>
    Response.json({ message: "rate limit" }, { status: 403 }),
  );
  const known = [release];
  const served = await publishedReleases(known);
  assert.deepEqual(
    served.map((r) => r.version),
    ["1.2.3"],
  );
  assert.equal(served[0].url, undefined);
});

test("any other lookup failure still fails the build", async (t) => {
  t.mock.method(globalThis, "fetch", async () =>
    Response.json({ message: "not found" }, { status: 404 }),
  );
  await assert.rejects(() => publishedReleases([]), /404/);
});

test("release data requires a stable identity and fixed text groups", () => {
  assert.equal(parseRelease(release, "1.2.3", "2026-09-15").title, "A release");
  assert.throws(() => parseRelease(release, "1.2.3-beta.1", "2026-09-15"));
  assert.throws(() =>
    parseRelease(
      { ...release, groups: { Fixes: "bad" } },
      "1.2.3",
      "2026-09-15",
    ),
  );
});
test("previews and drafts stay hidden while pagination finds stable releases", async (t) => {
  t.mock.method(globalThis, "fetch", async (url) => {
    if (new URL(url).searchParams.get("page") === "1")
      return Response.json(
        [
          { tag_name: "v1.3.0", draft: true },
          { tag_name: "v1.3.0-beta.1", prerelease: true },
        ],
        { headers: { link: '<next>; rel="next"' } },
      );
    if (new URL(url).searchParams.get("page") === "2")
      return Response.json([
        {
          tag_name: "v1.2.3",
          published_at: "2026-09-15",
          assets: [{ name: "release.json" }],
        },
      ]);
    return Response.json(release);
  });
  assert.deepEqual(
    (await publishedReleases([])).map((r) => r.version),
    ["1.2.3"],
  );
});

test("only releases read from GitHub link back to it", async (t) => {
  t.mock.method(globalThis, "fetch", async (url) => {
    if (new URL(url).hostname === "api.github.com")
      return Response.json([
        {
          tag_name: "v1.2.3",
          published_at: "2026-09-15",
          assets: [{ name: "release.json" }],
        },
      ]);
    return Response.json(release);
  });
  const served = await publishedReleases([
    { ...release, version: "1.2.2", publishedAt: "2026-09-01" },
  ]);
  const byVersion = new Map(served.map((r) => [r.version, r]));
  assert.equal(
    byVersion.get("1.2.3").url,
    "https://github.com/arocomputer/ulo/releases/tag/v1.2.3",
  );
  assert.equal(byVersion.get("1.2.2").url, undefined);
});
