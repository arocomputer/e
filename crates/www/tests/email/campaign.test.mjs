import assert from "node:assert/strict";
import test from "node:test";

// Dummy configuration is set before imports; every provider request is intercepted.
process.env.RESEND_API_KEY = "test-sending-key";
process.env.RESEND_CONTACTS_API_KEY = "test-contacts-key";
process.env.CONTACT_FROM = "sender@example.invalid";
process.env.MARKETING_POSTAL_ADDRESS = "Example sender address";
process.env.UNSUBSCRIBE_SECRET = "test-only-unsubscribe-secret";

const { sendCampaign } = await import("../../src/shared/email/campaign.ts");
const { prepareMarketingEmail, sendMarketingEmail } =
  await import("../../src/shared/email/marketing.ts");
const text = "Update. {{unsubscribe_url}} {{postal_address}}";

test("campaign preview reads subscribed recipients without sending a probe email", async (t) => {
  const requests = [];
  t.mock.method(globalThis, "fetch", async (url, options = {}) => {
    requests.push({ url, method: options.method || "GET" });
    return Response.json({
      data: [
        { email: "reader@example.invalid", unsubscribed: false },
        { email: "opted-out@example.invalid", unsubscribed: true },
        { email: "unknown-consent@example.invalid" },
      ],
    });
  });
  const report = await sendCampaign({
    subject: "Update",
    text,
    dryRun: true,
  });
  assert.deepEqual(requests, [
    { url: "https://api.resend.com/contacts", method: "GET" },
  ]);
  assert.deepEqual(report.recipients, ["reader@example.invalid"]);
  assert.equal(report.sent, 0);
  assert.deepEqual(report.failed, []);
});

test("every supplied message body must retain both required placeholders", (t) => {
  t.mock.method(console, "error", () => {});
  const result = prepareMarketingEmail({
    to: "reader@example.invalid",
    subject: "Update",
    text,
    html: "No opt-out",
  });
  assert.deepEqual(result, { ok: false, reason: "missing_unsubscribe" });
});

test("real sending uses the prepared body and signed unsubscribe headers", async (t) => {
  let sent;
  t.mock.method(globalThis, "fetch", async (url, options) => {
    assert.equal(url, "https://api.resend.com/emails");
    sent = JSON.parse(options.body);
    return Response.json({ id: "test-message" });
  });
  const result = await sendMarketingEmail({
    to: "reader@example.invalid",
    subject: "Update",
    text,
  });
  assert.deepEqual(result, { ok: true, id: "test-message" });
  assert.deepEqual(sent.to, ["reader@example.invalid"]);
  assert.match(sent.text, /unsubscribe\?token=/);
  assert.match(sent.text, /Example sender address/);
  assert.equal(
    sent.headers["List-Unsubscribe-Post"],
    "List-Unsubscribe=One-Click",
  );
});
