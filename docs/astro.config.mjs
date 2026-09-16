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
      components: {
        PageTitle: "./src/components/PageTitle.astro",
        SocialIcons: "./src/components/SocialIcons.astro",
      },
      // Code blocks are flat frames on the page surface, and long lines wrap
      // inside the column rather than scrolling out of it.
      expressiveCode: {
        defaultProps: { wrap: true, frame: "code" },
        styleOverrides: {
          borderRadius: "0",
          borderColor: "var(--sl-color-hairline-light)",
          codeBackground: "var(--sl-color-gray-6)",
          codeFontFamily: "var(--__sl-font-mono)",
          codeFontSize: "0.8125rem",
          codeLineHeight: "1.65",
          uiFontFamily: "var(--__sl-font)",
          frames: {
            shadowColor: "transparent",
            frameBoxShadowCssValue: "none",
            editorTabBarBackground: "var(--sl-color-gray-6)",
            editorActiveTabBackground: "var(--sl-color-gray-6)",
            editorActiveTabIndicatorTopColor: "transparent",
            editorTabBarBorderBottomColor: "var(--sl-color-hairline-light)",
            terminalBackground: "var(--sl-color-gray-6)",
            terminalTitlebarBackground: "var(--sl-color-gray-6)",
            terminalTitlebarDotsOpacity: "0",
            terminalTitlebarBorderBottomColor: "var(--sl-color-hairline-light)",
          },
        },
      },
    }),
  ],
});
