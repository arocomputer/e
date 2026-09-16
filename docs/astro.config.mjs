import { defineConfig } from "astro/config";
import { satteri } from "@astrojs/markdown-satteri";
import starlight from "@astrojs/starlight";

import { guidePlugins, sidebar } from "./src/guides.mjs";

/** e's docs site: the guides in ./guides, served under e.intuitum.sh/docs. */
const base = "/docs";

export default defineConfig({
  site: "https://e.intuitum.sh",
  base,
  markdown: {
    processor: satteri({ mdastPlugins: [guidePlugins(base)] }),
  },
  integrations: [
    starlight({
      title: "e",
      description: "The coding agent you can put anywhere.",
      logo: {
        light: "../assets/logo.svg",
        dark: "../assets/logo-dark.svg",
        replacesTitle: true,
      },
      favicon: "/favicon.svg",
      social: [
        { icon: "github", label: "GitHub", href: "https://github.com/intuitums/e" },
      ],
      editLink: { baseUrl: "https://github.com/intuitums/e/edit/main/docs/" },
      markdown: { processedDirs: ["./guides"] },
      sidebar: sidebar().map((group) => ({
        ...group,
        // The catalog is a page of this site, not a guide: it follows the
        // packages guide it belongs to.
        items: group.items.flatMap((item) =>
          item === "packages" ? [item, { label: "Package catalog", link: "/catalog/" }] : [item],
        ),
      })),
      customCss: ["@fontsource-variable/jetbrains-mono", "./src/styles/e.css"],
      components: { SocialIcons: "./src/components/SocialIcons.astro" },
    }),
  ],
});
