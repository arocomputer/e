import {
  groupDocs,
  groups,
  publicDocPath,
} from "@/sites/ulo/components/docs/data";

/** Public product facts and primary documentation for assistants and readers. */
export function GET() {
  return new Response(
    `# ulo

ulo is the coding agent you can put anywhere, built by Aro.
It ships as a Rust binary and supports multiple model providers, session trees,
automatic compaction, executable JSONL extensions, JSON/RPC, and a Rust SDK.
Tools run with the user's permissions. Directory trust is not a sandbox.

- [Home](https://ulo.sh/)
- [Changelog](https://ulo.sh/changelog)
- [Contact](https://ulo.sh/contact)

${groups
  .map(
    (group) =>
      `${group.title}:\n${groupDocs(group.slug)
        .map(
          (entry) =>
            `- [${entry.title}](https://ulo.sh${publicDocPath(entry.route)}): ${entry.description}`,
        )
        .join("\n")}`,
  )
  .join("\n")}
- [Legal](https://ulo.sh/legal)
- [Software license](https://ulo.sh/legal/license)
- [Privacy](https://ulo.sh/legal/privacy)
- [Safety and privacy](https://ulo.sh/legal/safety)
- [Source](https://github.com/arocomputer/ulo)
- [Terms](https://ulo.sh/legal/terms)
- [Report a vulnerability](https://ulo.sh/legal/report-vulnerability)
`,
    { headers: { "Content-Type": "text/plain" } },
  );
}
