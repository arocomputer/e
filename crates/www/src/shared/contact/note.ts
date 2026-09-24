/** The editable note validated by the contact form and the email endpoint. */
export type ContactNote = {
  name: string;
  email: string;
  subject: string;
  comments: string;
};

export const NOTE_LIMITS = {
  name: 200,
  email: 200,
  subject: 120,
  comments: 5000,
} as const;
export const EMPTY_NOTE: ContactNote = {
  name: "",
  email: "",
  subject: "",
  comments: "",
};

/** Validate the note again on the server; subject is optional. */
export function validateNote(note: ContactNote): string {
  if (!note.name.trim() || !note.email.trim() || !note.comments.trim()) {
    return "Please enter your name, email, and message.";
  }
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(note.email.trim())) {
    return "Please enter a valid email address.";
  }
  for (const field of Object.keys(NOTE_LIMITS) as (keyof ContactNote)[]) {
    if (note[field].trim().length > NOTE_LIMITS[field]) {
      return `${field === "comments" ? "Message" : field[0].toUpperCase() + field.slice(1)} is too long (${NOTE_LIMITS[field]} characters max).`;
    }
  }
  return "";
}

/** Suggest a subject locally. Visitors can replace it before sending. */
export function suggestSubject(message: string): string {
  const text = message.replace(/https?:\/\/\S+|\S+@\S+/gi, "");
  const products = ["Diffuse", "ulo"].filter((name) =>
    new RegExp(`\\b${name}\\b`, "i").test(text),
  );
  const product = products.length === 1 ? products[0] : "";
  if (/\bcollaborat\w*\b|\bwork(?:ing)? together\b/i.test(text))
    return "A potential collaboration";
  if (/\bfeature request\b|\b(?:could you|please) add\b/i.test(text))
    return product ? `Feature idea for ${product}` : "A feature suggestion";
  if (/\bbug\b|\bcrash\w*\b|\bnot working\b/i.test(text))
    return product ? `Bug report for ${product}` : "A bug report";
  if (/\bfeedback\b/i.test(text))
    return product ? `Feedback on ${product}` : "Feedback for Aro";
  if (/\bquestion\b|\bwondering\b|\?/i.test(text))
    return product ? `A question about ${product}` : "A question for Aro";
  if (/\bthanks\b|\bthank you\b/i.test(text)) return "A note of thanks";
  return product ? `A note about ${product}` : "A note for Aro";
}
