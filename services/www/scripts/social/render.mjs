/**
 * Render ulo's share card to `src/assets/social.png`, which `/social`
 * serves as-is. The layout is the one `next/og` drew for the Next site:
 * satori with Geist Regular (its default font, so weight 600 still draws
 * regular) and no kerning, like the satori it bundled, rasterized by resvg
 * at 1200×630. Run `npm run social` after changing the card and commit the
 * PNG.
 */
import { readFileSync, writeFileSync } from "node:fs";
import satori from "satori";
import { Resvg } from "@resvg/resvg-js";

const paper = "rgb(250 250 250)";
const ink = "rgb(36 36 36)";
const rule = "rgb(217 217 217)";

/** A satori element without JSX. */
const h = (type, style, ...children) => ({
  type,
  props: { style, children },
});

/** The supplied ulo wordmark at `height`, in the surrounding ink color. */
const logo = (height) => ({
  type: "svg",
  props: {
    width: height * 5,
    height,
    viewBox: "0 0 150 30",
    fill: "currentColor",
    children: [
      { type: "path", props: { d: "M10 30H0V0H10V30Z" } },
      { type: "path", props: { d: "M20 30V0H30V20H50V0H60V30H20Z" } },
      { type: "path", props: { d: "M65 30V0H75V20H105V30H65Z" } },
      {
        type: "path",
        props: { d: "M150 30V0H110V30H150ZM140 20H120V10H140V20Z" },
      },
    ],
  },
});

const card = h(
  "div",
  {
    background: paper,
    color: ink,
    display: "flex",
    width: "100%",
    height: "100%",
    padding: 32,
    fontFeatureSettings: '"kern" 0',
  },
  h(
    "div",
    {
      display: "flex",
      flexDirection: "column",
      width: "100%",
      border: `1px solid ${rule}`,
    },
    h(
      "div",
      {
        display: "flex",
        alignItems: "center",
        gap: 24,
        padding: "18px 32px",
        borderBottom: `1px solid ${rule}`,
      },
      logo(50),
      h("span", { fontSize: 20 }, "by Aro"),
    ),
    h(
      "div",
      { display: "flex", flex: 1, flexDirection: "column" },
      h(
        "div",
        {
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          gap: 16,
          flex: 1,
          padding: 32,
        },
        h("span", { fontSize: 58, fontWeight: 600 }, "The coding agent"),
        h("span", { fontSize: 58, fontWeight: 600 }, "you can put anywhere."),
      ),
      h(
        "div",
        {
          display: "flex",
          justifyContent: "space-between",
          padding: "24px 32px",
          borderTop: `1px solid ${rule}`,
          fontSize: 21,
          lineHeight: 1.6,
        },
        h("span", {}, "One Rust binary. Your choice of model."),
        h("span", {}, "ulo.sh"),
      ),
    ),
  ),
);

const font = readFileSync(new URL("Geist-Regular.ttf", import.meta.url));
const svg = await satori(card, {
  width: 1200,
  height: 630,
  fonts: [{ name: "geist", data: font, weight: 400, style: "normal" }],
});
const png = new Resvg(svg, { fitTo: { mode: "width", value: 1200 } })
  .render()
  .asPng();
writeFileSync(new URL("../../src/assets/social.png", import.meta.url), png);
