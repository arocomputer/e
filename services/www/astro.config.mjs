import { defineConfig } from "astro/config";
import cloudflare from "@astrojs/cloudflare";
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";

/** Normalize Astro's absolute module URLs for Vite; page URLs remain untouched. */
const devModules = {
  name: "astro-dev-modules",
  configureServer(server) {
    const root = fileURLToPath(new URL(".", import.meta.url));
    server.middlewares.use((request, _response, next) => {
      if (request.url?.startsWith(root))
        request.url = `/${request.url.slice(root.length)}`;
      next();
    });
  },
};

/** Pages and public assets use their native Astro routes in development and production. */
export default defineConfig({
  site: "https://ulo.sh",
  output: "static",
  trailingSlash: "never",
  build: { format: "file" },
  adapter: cloudflare({ imageService: "compile" }),
  integrations: [react()],
  vite: { plugins: [devModules, tailwindcss()] },
  devToolbar: { enabled: false },
});
