/**
 * Serve the built product locally, preserving explicit Host headers so tests
 * can exercise redirects from old bookmarks and the www hostname.
 * Extra arguments pass through to wrangler (`--port 8790`).
 */
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const source = "dist/server/wrangler.json";
const config = JSON.parse(readFileSync(source, "utf8"));
delete config.routes;
delete config.services;
const target = "dist/server/wrangler.preview.json";
writeFileSync(target, JSON.stringify(config));
const result = spawnSync(
  "npx",
  ["wrangler", "dev", "--config", target, ...process.argv.slice(2)],
  { stdio: "inherit" },
);
process.exit(result.status ?? 1);
