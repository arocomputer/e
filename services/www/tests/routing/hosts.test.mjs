import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { request as httpRequest } from "node:http";
import test from "node:test";

const origin = process.env.ROUTING_TEST_ORIGIN || "http://localhost:8787";

/** Read the built Worker with an explicit host, including redirect bodies. */
function page(path, host = "ulo.sh", method = "GET") {
  return new Promise((resolve, reject) => {
    const request = httpRequest(
      new URL(path, origin),
      { headers: { host }, method },
      (response) => {
        let body = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => (body += chunk));
        response.on("end", () => resolve({ response, body }));
      },
    );
    request.on("error", reject);
    request.setTimeout(10000, () =>
      request.destroy(new Error("Preview did not respond")),
    );
    request.end();
  });
}

test("the product serves its public pages and canonical URLs on its own host", async () => {
  for (const path of [
    "/",
    "/docs",
    "/docs/install",
    "/contact",
    "/legal/privacy",
  ]) {
    const { response, body } = await page(path);
    assert.equal(response.statusCode, 200, path);
    assert.ok(
      body.includes(`href="https://ulo.sh${path === "/" ? "" : path}"`),
      path,
    );
    assert.equal(response.headers["x-content-type-options"], "nosniff");
  }
});

test("old product and www bookmarks retain paths and queries", async () => {
  for (const [host, path] of [
    ["e.aro.computer", "/docs/install?from=old"],
    ["e.aro.computer", "/e/docs/install?from=old"],
    ["www.ulo.sh", "/docs/install?from=old"],
  ]) {
    const { response } = await page(path, host);
    assert.equal(response.statusCode, 308);
    assert.equal(
      response.headers.location,
      "https://ulo.sh/docs/install?from=old",
    );
  }
});

test("internal prefixes and retired guide names redirect without losing queries", async () => {
  for (const [path, destination] of [
    ["/ulo?from=docs", "/?from=docs"],
    ["/ulo/docs/install", "/docs/install"],
    ["/docs/", "/docs"],
    ["/docs/prompts", "/docs/prompt-templates"],
    ["/docs/customization", "/docs/customize"],
    ["/docs/safety", "/legal/safety"],
  ]) {
    const { response } = await page(path);
    assert.equal(response.statusCode, 308, path);
    assert.equal(response.headers.location, destination);
  }
});

test("legacy paths cannot redirect to another host through duplicate slashes", async () => {
  const { response } = await page("/ulo//example.com/path?from=old");
  assert.equal(response.statusCode, 308);
  assert.equal(response.headers.location, "/example.com/path?from=old");
});

test("the installer is exactly the script from this checkout", async () => {
  const expected = await readFile(
    new URL("../../../../install.sh", import.meta.url),
    "utf8",
  );
  const { response, body } = await page("/install.sh");
  assert.equal(response.statusCode, 200);
  assert.match(response.headers["content-type"], /text\/plain/);
  assert.equal(body, expected);
});

test("public assets and missing pages retain their status and content types", async () => {
  for (const [path, type] of [
    ["/favicon.ico", /svg/],
    ["/demo.mp4", /video\/mp4/],
    ["/.well-known/security.txt", /text\/plain/],
    ["/llms.txt", /text\/plain/],
    ["/robots.txt", /text\/plain/],
    ["/sitemap.xml", /xml/],
    ["/social", /image\/png/],
  ]) {
    const { response } = await page(path);
    assert.equal(response.statusCode, 200, path);
    assert.match(response.headers["content-type"], type, path);
  }
  const { response, body } = await page("/definitely-not-a-route");
  assert.equal(response.statusCode, 404);
  assert.ok(body.includes("ulo"));
});

test("unsubscribe confirmation remains on the service that signed the email", async () => {
  const { response } = await page("/unsubscribe?token=example");
  assert.equal(response.statusCode, 308);
  assert.equal(
    response.headers.location,
    "https://aro.computer/unsubscribe?token=example",
  );
});

test("contact uses email and retired form endpoints reject submissions", async () => {
  const { body } = await page("/contact");
  assert.match(body, /href="mailto:contact@aro\.computer"/);
  assert.doesNotMatch(body, /<form\b/);
  for (const [path, method] of [
    ["/api/contact", "POST"],
    ["/api/form-token", "GET"],
  ]) {
    const { response } = await page(path, "ulo.sh", method);
    assert.equal(response.statusCode, 410);
  }
});

test("one-click unsubscribe preserves the POST through its redirect", async () => {
  const { response } = await page(
    "/api/unsubscribe?token=example",
    "ulo.sh",
    "POST",
  );
  assert.equal(response.statusCode, 308);
  assert.equal(
    response.headers.location,
    "https://aro.computer/api/unsubscribe?token=example",
  );
});
