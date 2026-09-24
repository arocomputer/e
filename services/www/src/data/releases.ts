/** Published release assets are the shared source for GitHub and the website. */
export const groups = ["New features", "Improvements", "Fixes"] as const;
export type Release = {
  version: string;
  title: string;
  intro: string;
  publishedAt: string;
  groups: Record<(typeof groups)[number], string[]>;
  /**
   * The GitHub release page. Present only for entries read from a published
   * GitHub release; fallback entries have no release page to link.
   */
  url?: string;
};

/** Validate public data before rendering it; previews and HTML never become release entries. */
export function parseRelease(
  value: unknown,
  version: string,
  publishedAt: string,
): Release {
  if (!value || typeof value !== "object")
    throw new Error("Invalid release asset");
  const data = value as Record<string, unknown>;
  if (
    !/^\d+\.\d+\.\d+$/.test(version) ||
    data.version !== version ||
    typeof data.title !== "string" ||
    typeof data.intro !== "string" ||
    !Number.isFinite(Date.parse(publishedAt)) ||
    !data.groups ||
    typeof data.groups !== "object"
  ) {
    throw new Error("Invalid release identity");
  }
  const sections = data.groups as Record<string, unknown>;
  for (const group of groups) {
    if (
      !Array.isArray(sections[group]) ||
      !sections[group].every((item) => typeof item === "string")
    ) {
      throw new Error("Invalid release groups");
    }
  }
  return {
    version,
    publishedAt,
    title: data.title,
    intro: data.intro,
    groups: sections as Release["groups"],
  };
}

/**
 * Authenticate when a token is available. Anonymous callers get 60 requests an
 * hour per address, which a build shares with everything else on the machine,
 * so the changelog would fail for reasons that have nothing to do with it.
 */
function githubHeaders(): Record<string, string> {
  const headers: Record<string, string> = {
    Accept: "application/vnd.github+json",
  };
  const token = process.env.GITHUB_TOKEN;
  if (token) headers.Authorization = `Bearer ${token}`;
  return headers;
}

/**
 * Fetch stable releases only; follow pagination because dev builds share the
 * repository.
 *
 * A rate-limited list is not a bad changelog, so it returns what it already has
 * instead of failing the page: the supplied fallback, plus any pages read
 * before the limit. A later lookup corrects itself once the limit resets. Every
 * other failure still throws, because that is a real problem with the request.
 */
export async function publishedReleases(
  fallback: Release[],
): Promise<Release[]> {
  const result = new Map(fallback.map((release) => [release.version, release]));
  for (let page = 1; page <= 20; page++) {
    const response = await fetch(
      `https://api.github.com/repos/arocomputer/ulo/releases?per_page=100&page=${page}`,
      {
        headers: githubHeaders(),
        signal: AbortSignal.timeout(15000),
      },
    );
    if (response.status === 403 || response.status === 429) {
      console.warn(
        `Release lookup rate limited; serving ${result.size} known release(s)`,
      );
      break;
    }
    if (!response.ok)
      throw new Error(`Release lookup failed: ${response.status}`);
    const entries = await response.json();
    if (!Array.isArray(entries)) throw new Error("Invalid release list");
    for (const entry of entries) {
      if (
        entry.draft ||
        entry.prerelease ||
        !/^v\d+\.\d+\.\d+$/.test(entry.tag_name)
      )
        continue;
      if (
        !entry.assets?.some(
          (asset: { name: string }) => asset.name === "release.json",
        )
      )
        continue;
      const asset = await fetch(
        `https://github.com/arocomputer/ulo/releases/download/${entry.tag_name}/release.json`,
        {
          signal: AbortSignal.timeout(15000),
        },
      );
      if (!asset.ok)
        throw new Error(`Release asset unavailable: ${entry.tag_name}`);
      const release = parseRelease(
        await asset.json(),
        entry.tag_name.slice(1),
        entry.published_at,
      );
      release.url = `https://github.com/arocomputer/ulo/releases/tag/${entry.tag_name}`;
      result.set(release.version, release);
    }
    if (!response.headers.get("link")?.includes('rel="next"')) break;
  }
  return [...result.values()].sort(
    (a, b) => Date.parse(b.publishedAt) - Date.parse(a.publishedAt),
  );
}

/** How long a looked-up release list is served before the next lookup. */
const FRESH_MS = 300_000;

/**
 * `publishedReleases`, cached in the Workers Cache API for five minutes so the
 * changelog does not ask GitHub on every request. A failed refresh keeps
 * serving the last good list for up to a day, then the fallback, as ISR kept
 * the last rendered page. Without a Cache API (`astro dev`) every call looks
 * the releases up.
 */
export async function cachedReleases(fallback: Release[]): Promise<Release[]> {
  const key = new Request("https://ulo.sh/changelog/releases.json");
  let cache: Cache | undefined;
  let stored: { at: number; releases: Release[] } | undefined;
  try {
    cache = (caches as unknown as { default: Cache }).default;
    stored = await (await cache.match(key))?.json();
  } catch {
    cache = undefined;
  }
  if (stored && Date.now() - stored.at < FRESH_MS) return stored.releases;
  try {
    const releases = await publishedReleases(fallback);
    await cache
      ?.put(
        key,
        Response.json(
          { at: Date.now(), releases },
          { headers: { "cache-control": "max-age=86400" } },
        ),
      )
      .catch(() => undefined);
    return releases;
  } catch (error) {
    console.error("Release lookup failed; serving the last known list", error);
    return stored?.releases ?? fallback;
  }
}
