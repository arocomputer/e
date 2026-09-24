# Contact and email

Production requests reach the existing `aro` Worker through `CONTACT_SERVICE`.
It owns the credentials and rate limiter described below. The local handlers
remain available for development; setting secrets on the `ulo` Worker does
not replace the production service binding. Unsubscribe confirmation remains
at `https://aro.computer/unsubscribe`.

Aro lists a direct `mailto:contact@aro.computer` link instead of a form. ulo keeps
its product contact form and review flow through `/api/contact`, sharing
`src/shared/contact/note.ts` for field limits and validation.
ulo’s contact text fields retain their resting appearance on focus, with only the
text caret indicating editing.
Server delivery, subscriptions, and unsubscribe behavior use Resend.

ulo keeps the draft visible and editable before sending. The form retains failed
drafts, refreshes timing tokens after recoverable rejections, and prevents
duplicate submissions while a request is pending. Name, email, and message are
required; subject is optional. Subscription consent starts unchecked.

## Configuration

The endpoints read these from the Worker's environment; the build never sees
them. The sender and recipient addresses are plain `vars` in `wrangler.jsonc`;
everything else is a secret. Locally, copy the values you need from `.env.example` into `.dev.vars`
(git-ignored) before `npm run preview`, which copies it beside the built Worker.
In production, set each with `npx wrangler secret put NAME`. Keep real values
out of Git.

| Variable                   | Purpose                                                             |
| -------------------------- | ------------------------------------------------------------------- |
| `RESEND_API_KEY`           | Sending key, scoped to the verified sender domain                   |
| `CONTACT_FROM`             | Verified sender; required with the sending key                      |
| `CONTACT_TO`               | Recipient; defaults to `contact@aro.computer`                       |
| `FORM_TOKEN_SECRET`        | Contact timing-token key; falls back to `UNSUBSCRIBE_SECRET`        |
| `RESEND_CONTACTS_API_KEY`  | Contact-list access; falls back to the sending key                  |
| `UNSUBSCRIBE_SECRET`       | Signs unsubscribe links and success receipts                        |
| `SITE_URL`                 | Origin for outgoing email links; defaults to `https://aro.computer` |
| `MARKETING_FROM`           | Marketing sender; falls back to `CONTACT_FROM`                      |
| `MARKETING_REPLY_TO`       | Optional monitored address for replies                              |
| `MARKETING_POSTAL_ADDRESS` | Required address inserted into marketing messages                   |

A missing sending key or sender returns 503. Missing timing-token keys disable
that check. Contact attempts count in the `RateLimiter` Durable Object
(`src/shared/contact/rate-limiter.ts`): five per IP in a window that opens at the
first attempt and lasts ten minutes, shared by every Worker instance. Without
the binding (Node tests) the count is kept in memory; limiter errors are logged
and fail open. A Cloudflare rate limiting rule on the zone is the outer burst
limit. These are distinct failure modes, not equivalent protections.

## Submission contract

The handler limits request bytes before parsing, validates every editable field,
checks the hidden honeypot, verifies the timing token when configured, and limits
request frequency. Tokens allow ages from three seconds through six hours.
A token proves possession of a signed timestamp, not that a visitor is human.

The form reports success only after Resend accepts the notification. That confirms
provider acceptance, not delivery to the recipient's inbox. Opt-in is a separate,
unchecked choice. Subscription happens after the notification and may not turn an
accepted note into a failure. Store the email address without guessing a name.

For UI review, intercept success and error responses. An explicitly authorized
Resend delivery test can target `delivered@resend.dev` with a verified sender.
Ordinary development and browser checks must not send real mail.

## Subscriptions and campaigns

`src/shared/email/` owns subscriber updates, recipient-specific marketing messages,
campaign iteration, and signed unsubscribe links. The list key should have only
the contact permissions it needs; do not give the public sending key broader
permissions for convenience.

Campaigns include only contacts whose `unsubscribed` field is explicitly false.
Each recipient gets an individual message with signed unsubscribe headers.
Every provided HTML or text body must include `{{unsubscribe_url}}` and
`{{postal_address}}`. Missing required configuration or placeholders prevents
sending. Campaign requests are spaced by 600ms.

Review the recipient list and template before a real campaign. The campaign
preview must validate without sending a message. Sending requires explicit
user authorization.

`/unsubscribe` is the branded confirmation flow. `/api/unsubscribe` supports
one-click unsubscribe. Success screens require a receipt tied to the token;
changing a query string alone must not imply a successful unsubscribe.

## Operator

`src/shared/operator.ts` identifies the current operator as Fischer Hunt, doing
business as The Aro Computer Co. Keep legal-party names and outgoing postal
details accurate. Do not add a corporate suffix based on branding or an
assumption about incorporation.
