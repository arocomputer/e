/**
 * ulo guide paths renamed after publication. Keys and values are the clean
 * product URLs (`/docs/...`). `src/worker.ts` matches them as-is on ulo's host and
 * with an `/ulo` prefix on every other host, so both routes share one map.
 */
export const DOCS_ALIASES: Record<string, string> = {
  "/docs/customization": "/docs/customize",
  "/docs/prompts": "/docs/prompt-templates",
};
