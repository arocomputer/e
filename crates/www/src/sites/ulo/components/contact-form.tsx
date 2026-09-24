import { useEffect, useRef, useState, type SubmitEvent } from "react";
import {
  NOTE_LIMITS,
  validateNote,
  type ContactNote,
} from "@/shared/contact/note";

/** Reuse the protected contact API; retain the draft on failure and require explicit mailing-list consent. */
export function ContactForm() {
  const [status, setStatus] = useState<"idle" | "sending" | "sent">("idle");
  const [error, setError] = useState("");
  const token = useRef<string | null>(null);
  const sending = useRef(false);

  async function refreshToken() {
    try {
      const response = await fetch("/api/form-token", {
        cache: "no-store",
        signal: AbortSignal.timeout(10_000),
      });
      const data = response.ok
        ? await response.json<{ token?: string }>()
        : null;
      token.current = data?.token ?? null;
    } catch {
      token.current = null;
    }
  }

  useEffect(() => {
    void refreshToken();
  }, []);

  async function submit(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (sending.current) return;
    const fields = new FormData(event.currentTarget);
    const note: ContactNote = {
      name: String(fields.get("name") ?? ""),
      email: String(fields.get("email") ?? ""),
      subject: String(fields.get("subject") ?? "").trim() || "A note about ulo",
      comments: String(fields.get("comments") ?? ""),
    };
    const validation = validateNote(note);
    if (validation) {
      setError(validation);
      return;
    }
    sending.current = true;
    setStatus("sending");
    setError("");
    try {
      const response = await fetch("/api/contact", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          ...note,
          subscribe: fields.get("subscribe") === "yes",
          company_website: fields.get("company_website"),
          form_token: token.current,
        }),
        signal: AbortSignal.timeout(20_000),
      });
      const data = await response
        .json<{ ok?: boolean; error?: string; retryToken?: boolean }>()
        .catch(() => null);
      if (!response.ok || data?.ok !== true) {
        if (data?.retryToken) await refreshToken();
        throw new Error(
          data?.error || "Could not send your message. Please try again.",
        );
      }
      setStatus("sent");
    } catch (cause) {
      setStatus("idle");
      setError(
        cause instanceof Error && cause.name === "Error"
          ? cause.message
          : "Could not confirm delivery. Your draft is still here; try again or email us directly.",
      );
    } finally {
      sending.current = false;
    }
  }

  return (
    <div className="ulo-contact-form">
      {status === "sent" ? (
        <div className="ulo-contact-sent" role="status">
          <h2>Message sent.</h2>
          <p>We&apos;ll reply to the email address you provided.</p>
        </div>
      ) : (
        <form
          onSubmit={submit}
          action="mailto:contact@aro.computer"
          method="post"
          encType="text/plain"
          aria-label="Contact ulo"
        >
          <fieldset disabled={status === "sending"}>
            <div className="ulo-contact-pair">
              <label>
                Name
                <input
                  name="name"
                  autoComplete="name"
                  required
                  maxLength={NOTE_LIMITS.name}
                />
              </label>
              <label>
                Email
                <input
                  name="email"
                  type="email"
                  autoComplete="email"
                  required
                  maxLength={NOTE_LIMITS.email}
                />
              </label>
            </div>
            <label>
              Subject <span>Optional</span>
              <input name="subject" maxLength={NOTE_LIMITS.subject} />
            </label>
            <label>
              Message
              <textarea
                name="comments"
                required
                rows={7}
                maxLength={NOTE_LIMITS.comments}
              />
            </label>
            <div className="ulo-contact-honeypot" aria-hidden="true">
              <label>
                Company website
                <input
                  name="company_website"
                  tabIndex={-1}
                  autoComplete="off"
                />
              </label>
            </div>
            <label className="ulo-contact-consent">
              <input type="checkbox" name="subscribe" value="yes" />
              <span>Email me Aro updates. Unsubscribe anytime.</span>
            </label>
            <p className="ulo-contact-policy">
              Messages are handled under the{" "}
              <a href="/ulo/legal/privacy">Privacy Policy</a>.
            </p>
            <button className="ulo-button ulo-button-primary" type="submit">
              {status === "sending" ? "Sending…" : "Send message"}
            </button>
          </fieldset>
          <p className="ulo-contact-error" role="alert">
            {error}
          </p>
          <span className="ulo-visually-hidden" role="status">
            {status === "sending" ? "Sending message" : ""}
          </span>
        </form>
      )}
    </div>
  );
}
