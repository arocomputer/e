import assert from "node:assert/strict";
import test from "node:test";
import {
  NOTE_LIMITS,
  suggestSubject,
  validateNote,
} from "../../src/shared/contact/note.ts";

const note = {
  name: "Sam",
  email: "sam@example.com",
  subject: "",
  comments: "Hello.",
};

// A note accepts a single name and requires the same reply details.
test("a note needs a name, valid email, and message, but no subject or workplace", () => {
  assert.equal(validateNote(note), "");
  for (const field of ["name", "email", "comments"]) {
    assert.notEqual(validateNote({ ...note, [field]: "   " }), "");
  }
  assert.match(validateNote({ ...note, email: "sam@" }), /valid email/);
});

// The server and browser share the limits that bound an outgoing email.
for (const [field, limit] of Object.entries(NOTE_LIMITS)) {
  test(`${field} accepts its limit and rejects one more character`, () => {
    const value =
      field === "email"
        ? `${"a".repeat(limit - 12)}@example.com`
        : "a".repeat(limit);
    assert.equal(validateNote({ ...note, [field]: value }), "");
    assert.match(validateNote({ ...note, [field]: `a${value}` }), /too long/);
  });
}

test("a product mentioned only in a URL does not become the subject", () => {
  assert.equal(
    suggestSubject("Here's a link: https://example.com/diffuse"),
    "A note for Aro",
  );
});

test("a product bug gets an editable, product-specific subject", () => {
  assert.equal(
    suggestSubject("Diffuse crashes when I open it."),
    "Bug report for Diffuse",
  );
});
