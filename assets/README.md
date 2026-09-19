# e artwork

- [icon.svg](icon.svg): the light mark on a dark rounded square, matching the
  favicon on e.aro.computer.
- [logo.svg](logo.svg): the standalone three-rectangle mark in `#252525` on a
  transparent background. Set its fill to `currentColor` when embedding inline
  if it should inherit the surrounding text color.
- [logo-dark.svg](logo-dark.svg): the same mark in `#fafafa`, for dark
  backgrounds where inline color is not available — the README's
  `<picture>` switches to it under `prefers-color-scheme: dark`.

Keep the proportions and spacing intact. Scale the SVGs rather than redrawing
the rectangles. The website copies live in the arocomputer/web repository; keep
them aligned when changing the artwork.

## Themes

The two bundled palettes live in `crates/core/themes/`, beside the core that
embeds them for `e docs theme-dark` and hands them to the terminal frontend.

## README screenshot

[readme.png](readme.png) is a frame at 26 seconds from the
[website demo](https://e.aro.computer/e/demo.mp4). The recorded session uses
GPT-5.6 Sol with low reasoning effort to fix a JavaScript slug formatter and
run its tests. The model and effort appear in the terminal's status row.

The terminal text is unchanged. [readme-window.html](readme-window.html)
adds a compact title bar, window controls, and a border. There is no outer
backdrop or shadow.

The canvas exports the image with an alpha channel, so the rounded corners
stay transparent on both light and dark pages. Browser screenshots flatten
those corners onto the browser's default background; use the PNG export.

To regenerate it from the repository root, with ffmpeg and agent-browser:

```sh
curl -fsSL https://e.aro.computer/e/demo.mp4 -o /tmp/e-demo.mp4
mkdir -p target
ffmpeg -y -ss 26 -i /tmp/e-demo.mp4 -frames:v 1 target/readme-frame.png
python3 -m http.server 8769 --bind 127.0.0.1
```

In another terminal, open the frame and export its canvas once it has loaded:

```sh
agent-browser --session readme open http://127.0.0.1:8769/assets/readme-window.html
agent-browser --session readme wait 'canvas[data-ready="true"]'
agent-browser --session readme eval 'document.querySelector("canvas").toDataURL("image/png")' --json > /tmp/e-readme-png.json
python3 -c 'import base64,json,pathlib; data=json.load(open("/tmp/e-readme-png.json"))["data"]["result"]; pathlib.Path("assets/readme.png").write_bytes(base64.b64decode(data.split(",",1)[1]))'
agent-browser --session readme close
```

Stop the local HTTP server when finished.
