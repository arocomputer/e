import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { publishPackages } from "./publish-npm.mjs";

/** Model the immutable tarball and registry responses without publishing test packages. */
function fixture(t) {
    const root = mkdtempSync(join(tmpdir(), "e-npm-publish-"));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    mkdirSync(join(root, "e"));
    writeFileSync(
        join(root, "e/package.json"),
        JSON.stringify({ name: "@intuitums/e", version: "1.2.3" }),
    );
    writeFileSync(join(root, "e.tgz"), "tarball");
    const integrity =
        "sha512-" + createHash("sha512").update("tarball").digest("base64");
    const calls = [];
    const npm = (...args) => {
        calls.push(args);
        return JSON.stringify([{ filename: "e.tgz" }]);
    };
    return { root, integrity, calls, npm };
}

test("retry accepts the already-published tarball without publishing twice", async (t) => {
    const f = fixture(t);
    await publishPackages(f.root, {
        npm: f.npm,
        lookup: async () => ({ dist: { integrity: f.integrity } }),
    });
    assert.deepEqual(
        f.calls.map((args) => args[0]),
        ["pack"],
    );
});

test("publishing an older missing version cannot move latest backward", async (t) => {
    const f = fixture(t);
    let published = false;
    await publishPackages(f.root, {
        npm: (...args) => {
            if (args[0] === "publish") published = true;
            return f.npm(...args);
        },
        lookup: async (_, version) =>
            version === "latest"
                ? { version: "2.0.0" }
                : published
                  ? { dist: { integrity: f.integrity } }
                  : null,
    });
    const command = f.calls.find((args) => args[0] === "publish");
    assert.equal(command[command.indexOf("--tag") + 1], "v1.2.3");
});

test("a different tarball under the same version fails without overwriting it", async (t) => {
    const f = fixture(t);
    await assert.rejects(
        publishPackages(f.root, {
            npm: f.npm,
            lookup: async () => ({ dist: { integrity: "wrong" } }),
            sleep: async () => {},
        }),
        /does not match/,
    );
    assert.deepEqual(
        f.calls.map((args) => args[0]),
        ["pack"],
    );
});

 test("an older beta retry cannot move beta backward or touch latest", async (t) => {
    const f = fixture(t);
    const version = "1.2.3-beta.9.gabcdef012345";
    writeFileSync(join(f.root, "e/package.json"), JSON.stringify({name:"@intuitums/e", version, publishConfig:{tag:"beta"}}));
    let published = false;
    const lookups = [];
    await publishPackages(f.root, {
        npm: (...args) => {
            if (args[0] === "publish") published = true;
            return f.npm(...args);
        },
        lookup: async (_, requested) => {
            lookups.push(requested);
            if (requested === "beta") return {version:"1.2.3-beta.12.gabcdef012345"};
            return published ? {dist:{integrity:f.integrity}} : null;
        },
    });
    const command = f.calls.find(args => args[0] === "publish");
    assert.equal(command[command.indexOf("--tag") + 1], `v${version}`);
    assert.ok(!lookups.includes("latest"));
});
