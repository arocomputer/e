import { defineCollection } from "astro:content";
import { docsSchema } from "@astrojs/starlight/schema";

import { guidesLoader } from "./guides.mjs";

/**
 * Every guide in docs/guides/<group>/, keyed by its file stem so a page's URL
 * is its `e docs` topic. Group READMEs and example folders are not pages.
 */
export const collections = {
  docs: defineCollection({
    loader: guidesLoader(),
    schema: docsSchema(),
  }),
};
