/** Route ulo's public URLs to Astro, preserving old links and shared contact delivery. */
import astro from "@astrojs/cloudflare/entrypoints/server";
import { DOCS_ALIASES } from "./sites/ulo/data/docs-aliases";

export { RateLimiter } from "./shared/contact/rate-limiter";

const SECURITY_HEADERS: Record<string, string> = {
  "x-content-type-options": "nosniff",
  "x-frame-options": "DENY",
  "referrer-policy": "strict-origin-when-cross-origin",
  "permissions-policy":
    "camera=(), microphone=(), geolocation=(), browsing-topics=()",
  "strict-transport-security": "max-age=63072000",
};

/** Apply the same headers to pages, redirects, errors, and shared API replies. */
function secure(response: Response): Response {
  const headers = new Headers(response.headers);
  for (const [name, value] of Object.entries(SECURITY_HEADERS))
    headers.set(name, value);
  return new Response(response.body, { status: response.status, headers });
}

/** Preserve the query string when canonicalizing a hostname or path. */
function redirect(url: URL, destination: string): Response {
  return secure(
    new Response("Redirecting...", {
      status: 308,
      headers: {
        location: destination + url.search,
        "cache-control": "public, max-age=3600",
      },
    }),
  );
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext) {
    const url = new URL(request.url);
    const host = request.headers.get("host")?.split(":")[0].toLowerCase();
    // These names exist only to carry bookmarks from the previous product name.
    if (host === "e.aro.computer" || host === "www.ulo.sh") {
      const path = url.pathname.replace(/^\/(?:e|ulo)(?=\/|$)/, "") || "/";
      return redirect(url, `https://ulo.sh${path}`);
    }
    if (url.pathname === "/ulo" || url.pathname.startsWith("/ulo/")) {
      return redirect(url, url.pathname.slice(4) || "/");
    }
    if (url.pathname.length > 1 && url.pathname.endsWith("/")) {
      return redirect(url, url.pathname.replace(/\/+$/, ""));
    }
    const alias = DOCS_ALIASES[url.pathname];
    if (alias) return redirect(url, alias);
    if (url.pathname === "/docs/safety") return redirect(url, "/legal/safety");

    // A service binding retains the existing signing keys, consent handling,
    // and per-IP limiter in Aro. Local previews use the checked-in handlers.
    if (
      ["/api/contact", "/api/form-token", "/api/unsubscribe"].includes(
        url.pathname,
      ) &&
      env.CONTACT_SERVICE
    ) {
      url.hostname = "aro.computer";
      return secure(await env.CONTACT_SERVICE.fetch(new Request(url, request)));
    }
    if (url.pathname === "/unsubscribe")
      return redirect(url, "https://aro.computer/unsubscribe");

    const isApi = url.pathname.startsWith("/api/");
    const isAsset = url.pathname.startsWith("/_astro/");
    if (!isApi && !isAsset) {
      url.pathname =
        url.pathname === "/favicon.ico"
          ? "/ulo/icon.svg"
          : `/ulo${url.pathname === "/" ? "" : url.pathname}`;
      request = new Request(url, request);
    }
    let response = await astro.fetch(request, env, ctx);
    if (response.status === 404 && !isApi && !isAsset) {
      const page = await env.ASSETS.fetch(new URL("/ulo/404", url));
      response = new Response(page.body, {
        status: 404,
        headers: page.headers,
      });
    }
    return secure(response);
  },
} satisfies ExportedHandler<Env>;
