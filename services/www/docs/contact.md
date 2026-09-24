# Contact

`src/pages/contact.astro` links to `mailto:contact@aro.computer`. Email opens
in the visitor's mail client. The website has no contact form, subscriber
list, sending credentials, or contact-service binding.

`/api/contact` and `/api/form-token` return 410 for clients of the retired
form. Old `/unsubscribe` and `/api/unsubscribe` links redirect to the Aro
website, which issued those emails and can validate their tokens. Preserve
the query and POST method when redirecting those links.

`src/data/operator.ts` identifies the legal operator. A standalone deployment
does not change the legal party or the monitored contact addresses.
