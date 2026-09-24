/** Serve native Astro routes with security headers and redirects for existing bookmarks. */
import { DOCS_ALIASES } from "./data/docs-aliases";

const SECURITY_HEADERS: Record<string, string> = {
  "x-content-type-options": "nosniff",
  "x-frame-options": "DENY",
  "referrer-policy": "strict-origin-when-cross-origin",
  "permissions-policy":
    "camera=(), microphone=(), geolocation=(), browsing-topics=()",
  "strict-transport-security": "max-age=63072000",
};

/** Apply the same headers to pages, redirects, and errors. */
function secure(response: Response): Response {
  const headers = new Headers(response.headers);
  for (const [name, value] of Object.entries(SECURITY_HEADERS))
    headers.set(name, value);
  return new Response(response.body, { status: response.status, headers });
}

/** Preserve the query string when canonicalizing a hostname or path. */
function redirect(url: URL, destination: string): Response {
  // A stripped legacy prefix must not turn a path into a protocol-relative URL.
  if (destination.startsWith("/"))
    destination = destination.replace(/^\/+/, "/");
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
    const host = (request.headers.get("host") ?? url.hostname)
      .split(":")[0]
      .toLowerCase();
    // These names exist only to carry bookmarks from the previous product name.
    if (host === "e.aro.computer" || host === "www.ulo.sh") {
      const path = url.pathname.replace(/^\/(?:e|ulo)(?=\/|$)/, "") || "/";
      return redirect(url, `https://ulo.sh${path}`);
    }
    if (url.pathname === "/ulo" || url.pathname.startsWith("/ulo/")) {
      return redirect(url, url.pathname.slice(4) || "/");
    }
    if (url.pathname.length > 1 && url.pathname.endsWith("/")) {
      return redirect(url, url.pathname.replace(/\/+$/, "") || "/");
    }
    const alias = DOCS_ALIASES[url.pathname];
    if (alias) return redirect(url, alias);
    if (url.pathname === "/docs/safety") return redirect(url, "/legal/safety");

    // Existing email links must reach the service that signed their tokens.
    if (["/unsubscribe", "/api/unsubscribe"].includes(url.pathname))
      return redirect(url, `https://aro.computer${url.pathname}`);
    if (["/api/contact", "/api/form-token"].includes(url.pathname))
      return secure(
        Response.json(
          { error: "The contact form has closed. Email contact@aro.computer." },
          { status: 410 },
        ),
      );

    const isApi = url.pathname.startsWith("/api/");
    const isAsset = url.pathname.startsWith("/_astro/");
    if (url.pathname === "/favicon.ico") {
      url.pathname = "/icon.svg";
      request = new Request(url, request);
    }
    // Resolve the current renderer per request; hot reload can replace its React modules.
    const { default: astro } =
      await import("@astrojs/cloudflare/entrypoints/server");
    let response = await astro.fetch(request, env, ctx);
    if (response.status === 404 && !isApi && !isAsset) {
      // The development entrypoint resolves pages, while ASSETS owns public files.
      const asset = await env.ASSETS.fetch(request);
      if (asset.status !== 404) response = asset;
      else {
        const page = await astro.fetch(
          new Request(new URL("/404", url)),
          env,
          ctx,
        );
        response = new Response(page.body, {
          status: 404,
          headers: page.headers,
        });
      }
    }
    return secure(response);
  },
} satisfies ExportedHandler<Env>;
