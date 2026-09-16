# e artwork

- [icon.svg](icon.svg): the light mark on a dark rounded square, matching the
  favicon on e.intuitum.sh.
- [logo.svg](logo.svg): the standalone three-rectangle mark in `#252525` on a
  transparent background. Set its fill to `currentColor` when embedding inline
  if it should inherit the surrounding text color.
- [logo-dark.svg](logo-dark.svg): the same mark in `#fafafa`, for dark
  backgrounds where inline color is not available — the README's
  `<picture>` switches to it under `prefers-color-scheme: dark`.

Keep the proportions and spacing intact. Scale the SVGs rather than redrawing
the rectangles. The website copies live in the intuitum-sh repository at
`public/e/icon.svg` and `src/app/(sub)/e/brand-mark.tsx`; keep them aligned when
changing the artwork.

## Themes

The two bundled palettes live in `crates/core/themes/`, beside the core that
embeds them for `e docs theme-dark` and hands them to the terminal frontend.

## README screenshot

[readme.png](readme.png) is a frame at 26 seconds from the
[website demo](https://e.intuitum.sh/e/demo.mp4). The recorded session uses
GPT-5.6 Sol with low reasoning effort to fix a JavaScript slug formatter and
run its tests. The model and effort appear in the terminal's status row.

The frame is extracted directly from the recording, with no added background,
window controls, or changes to the terminal text:

```sh
curl -fsSL https://e.intuitum.sh/e/demo.mp4 -o /tmp/e-demo.mp4
ffmpeg -y -ss 26 -i /tmp/e-demo.mp4 -frames:v 1 assets/readme.png
```
