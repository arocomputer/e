# Writing e's documentation

`docs/guides/` is the only copy of e's guides. Three readers render them:

- **GitHub** — the files themselves, as you see them here.
- **`e docs <topic>`** — the binary embeds the `.md` files, so a guide ships with
  the release it documents and the agent can read it without a network.
- **e.intuitum.sh/docs** — the website, in intuitums/web, renders each guide as
  a page. Its `scripts/e/docs/docs.mjs` is the one place that adapts the guides
  to it: the sidebar, link routes, and alerts.

Write once, and all three follow. Never paste a guide's text into another
page, the README, or an issue: link to it.

## Layout

```
docs/
  README.md              this file — GitHub only, never a topic and never a page
  src/                   the guides adapter, the palette, and site-only pages
  guides/
    start/               one folder per nav group
      README.md          the group's label and order, and nothing else
      getting-started.md the website's landing page
    usage/
      README.md
      sessions.md
    customize/
      README.md
      settings.md
    extend/
      README.md
      examples/          assets a guide links to (code, images); not topics
contributing/          the repository's own documentation: architecture,
                       rendering, releases. Never on the website, never in
                       `e docs`.
```

- **The folder is the nav group**, and its README.md's front matter names it
  and orders it: `docs/guides/usage/` is “Usage”, second in the sidebar. Nothing is
  numbered, so renaming a group is renaming a folder.
- **A group has a subject.** `start/` is the first run, `usage/` is day-to-day
  operation, `customize/` is the `~/.e` surface, and `extend/` is building
  against e. Put a guide where a reader would look for it, and move it when
  that changes.
- **The file stem is the `e docs` topic.** `docs/guides/customize/themes.md` is
  `e docs themes`. Stems are unique across the whole folder tree.
- **The guide named `getting-started` is the website's landing page.**
  `e.intuitum.sh/docs` renders that file, so its first commands are the ones a
  new reader copies, and a change there needs the same care as a change to the
  install script.
- **A folder may hold assets** beside its guides — an example, an image. They
  stay beside the guide that links them, and the site links them on GitHub;
  only `.md` files become pages.
- **`README.md` in any folder is repository-only.** The one at the root is this
  guide; a group's carries that group's front matter.

## Front matter

Every guide starts with it, and it is the only metadata:

```md
---
title: Themes
description: theme JSON format; file wins over a built-in name
order: 5
---
```

- `title` — the label in the site's navigation and the page's heading.
- `description` — one line, no trailing period needed. It is the site's
  meta description and the blurb `e docs` prints when listing topics.
- `order` — position inside the group. Gaps are fine; ties fall back to the
  file name. In a group's README.md the same key orders the *group*.

A group README carries the same three keys and no guide content; the site
uses it for the sidebar label and position.

Only those three keys, one line each, no nested YAML. `crates/core/build.rs` and the
site both parse them with a few lines of string handling, deliberately not a
YAML dependency.

## Markdown and the heading

- **A guide is `.md`**, the one format all three readers take. A page that
  needs components, like the package catalog, is an Astro page under
  `src/pages/`: part of the site, not a guide, and not in `e docs`.
- **Keep the `# Title` heading.** GitHub needs it; the site renders the
  front matter's `title` and drops the duplicate heading.
- **A note that must stand out uses GitHub's alert syntax**, which GitHub
  renders as an alert and the site renders as an aside — no component, no
  change to the site. `e docs` prints the label instead of the marker
  (`> Warning:`), because `[!WARNING]` is not prose to a reader in a shell:

  ```md
  > [!WARNING]
  > A package runs with your full permissions.
  ```

  The labels are `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, and
  `[!CAUTION]`.

## Links

- **Another guide:** link it relatively — `[themes](guides/customize/themes.md)` from
  this folder, or `themes.md` from beside it. GitHub resolves either, and the
  site rewrites it to the page's route. Do not write repository-absolute
  paths like `/docs/guides/customize/themes.md`: they break on GitHub.
- **Anything outside `docs/guides/`** — `contributing/`, `crates/`, an example file —
  link it relatively too. The site points those at GitHub, since they are
  not pages.
- **Fragments work** (`extensions.md#results-by-method`) and the site keeps
  them.

## Checking your work

`./x docs` covers `docs/guides/`: every guide has complete front matter, the
topic names are unique, every relative link resolves, and `e docs` serves every
topic. No network, no build.

To read the pages as they will appear, build the website against this
checkout: in intuitums/web, run `E_DOCS_PATH=<path to this checkout> npm run
preview`. Merging a guide to `main` redeploys the website
(`.github/workflows/docs.yml`).

## Adding a guide

1. Put it in the group it belongs to, and name the file after its topic.
2. Add front matter with `title`, `description`, and `order`.
3. Link it from a nearby guide — the navigation follows the folders, so a new
   page appears without another list to edit.
4. Run `./x docs`.

A new group is a new folder with a `README.md` whose front matter gives its
`title` and `order`; the sidebar and the topics list pick it up from there.
