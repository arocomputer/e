import { defineConfig } from "astro/config";
import cloudflare from "@astrojs/cloudflare";
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const INTERNAL_PREFIXES = [
  "/ulo",
  "/api",
  "/_astro",
  "/@vite",
  "/@fs",
  "/node_modules",
  "/src",
];
/** Astro asks for a page's own `<script>` by absolute path; Vite wants it relative to the root. */
const PROJECT_ROOT = fileURLToPath(new URL(".", import.meta.url)).replace(
  /\/$/,
  "",
);
const PUBLIC_PATHS = new Set([
  "/.well-known/security.txt",
  "/favicon.ico",
  "/icon.svg",
]);

/** Give Vite the Worker's product prefix while serving pages and public files. */
const uloDevRoutes = {
  name: "ulo-dev-routes",
  configureServer(server) {
    server.middlewares.use(async (request, response, next) => {
      if (!request.url) return next();
      const requestUrl = new URL(request.url, "http://localhost");
      if (requestUrl.pathname.startsWith(`${PROJECT_ROOT}/`)) {
        request.url = `${requestUrl.pathname.slice(PROJECT_ROOT.length)}${requestUrl.search}`;
        return next();
      }
      if (
        requestUrl.pathname.startsWith("/@") ||
        INTERNAL_PREFIXES.some(
          (prefix) =>
            requestUrl.pathname === prefix ||
            requestUrl.pathname.startsWith(`${prefix}/`),
        )
      )
        return next();

      if (PUBLIC_PATHS.has(requestUrl.pathname)) {
        try {
          const body = await readFile(
            new URL(
              `./public/ulo${requestUrl.pathname === "/favicon.ico" ? "/icon.svg" : requestUrl.pathname}`,
              import.meta.url,
            ),
          );
          const extension =
            requestUrl.pathname === "/favicon.ico"
              ? "svg"
              : requestUrl.pathname.split(".").at(-1);
          const contentTypes = {
            ico: "image/x-icon",
            png: "image/png",
            svg: "image/svg+xml",
            txt: "text/plain; charset=utf-8",
          };
          response.statusCode = 200;
          response.setHeader(
            "content-type",
            contentTypes[extension] ?? "application/octet-stream",
          );
          response.end(body);
          return;
        } catch (error) {
          next(error);
          return;
        }
      }

      request.url = `/ulo${requestUrl.pathname === "/" ? "" : requestUrl.pathname}${requestUrl.search}`;
      next();
    });
  },
};

/**
 * The product site lives under `src/pages/ulo`; the Worker maps public
 * URLs onto that internal prefix. Everything prerenders to static assets
 * except endpoints that mark `prerender = false`.
 */
export default defineConfig({
  site: "https://ulo.sh",
  output: "static",
  trailingSlash: "never",
  build: { format: "file" },
  adapter: cloudflare({ imageService: "compile" }),
  integrations: [react()],
  vite: { plugins: [uloDevRoutes, tailwindcss()] },
  // Mail clients send one-click unsubscribe (RFC 8058) as a cross-site form
  // POST; Astro's origin check would reject it. The contact API takes JSON
  // with a signed form token, so it does not rely on this check.
  security: { checkOrigin: false },
  devToolbar: { enabled: false },
});
