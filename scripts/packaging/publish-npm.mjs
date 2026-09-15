import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const runNpm = (...args) => execFileSync("npm", args, { encoding: "utf8" });
const newer = (a, b) => {
    const parts = (v) => v.split(/[.-]/).slice(0, 5).map((s, i) => i === 3 ? s : Number(s));
    const left = parts(a), right = parts(b);
    for (const i of [0, 1, 2, 4])
        if ((left[i] ?? 0) !== (right[i] ?? 0)) return left[i] > right[i];
    return false;
};
async function lookupRegistry(name, version) {
    const response = await fetch(
        `https://registry.npmjs.org/${encodeURIComponent(name)}/${version}`,
        { signal: AbortSignal.timeout(30000) },
    );
    if (response.status === 404) return null;
    if (!response.ok)
        throw new Error(`npm lookup failed: HTTP ${response.status}`);
    return response.json();
}
/** Publish exact packages once; injectable registry/CLI calls keep retry tests offline. */
export async function publishPackages(
    root,
    {
        npm = runNpm,
        lookup = lookupRegistry,
        sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    } = {},
) {
    const folders = readdirSync(root, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .map((entry) => entry.name)
        .sort((a, b) => (a === "e" ? 1 : b === "e" ? -1 : a.localeCompare(b)));
    for (const folder of folders) {
        const path = resolve(root, folder);
        const manifest = JSON.parse(
            readFileSync(resolve(path, "package.json"), "utf8"),
        );
        const [packed] = Object.values(
            JSON.parse(
                npm(
                    "pack",
                    path,
                    "--json",
                    "--ignore-scripts",
                    "--pack-destination",
                    root,
                ),
            ),
        );
        const tarball = resolve(root, packed.filename);
        const integrity =
            "sha512-" +
            createHash("sha512").update(readFileSync(tarball)).digest("base64");
        for (let attempt = 0; ; attempt++) {
            try {
                let existing = await lookup(manifest.name, manifest.version);
                if (!existing) {
                    console.log(
                        `Publishing ${manifest.name}@${manifest.version}`,
                    );
                    const channel = manifest.publishConfig?.tag ?? "latest";
                    const latest = await lookup(manifest.name, channel);
                    const tag =
                        latest && newer(latest.version, manifest.version)
                            ? `v${manifest.version}`
                            : channel;
                    npm(
                        "publish",
                        tarball,
                        "--access",
                        "public",
                        "--tag",
                        tag,
                        "--ignore-scripts",
                    );
                    existing = await lookup(manifest.name, manifest.version);
                }
                if (existing?.dist?.integrity !== integrity)
                    throw new Error(
                        `Published content does not match ${manifest.name}@${manifest.version}`,
                    );
                break;
            } catch (error) {
                if (attempt === 2) throw error;
                await sleep(5000 * (attempt + 1));
            }
        }
        console.log(`Verified ${manifest.name}@${manifest.version}`);
    }
}

if (
    process.argv[1] &&
    resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
    await publishPackages(resolve(process.argv[2]));
}
