# Terminal demo

The homepage plays `public/demo.mp4` through a native video element. Source
terminal cells stay in the export tools and are not loaded by visitors.

## Source

| File                                        | Purpose                                               |
| ------------------------------------------- | ----------------------------------------------------- |
| `src/components/demo/player.tsx`            | Native media clock, visibility, and controls          |
| `src/styles/demo.css`                       | Video sizing and playback line                        |
| `src/components/demo/export/recording.json` | Reviewed cells captured from the real ulo TUI         |
| `src/components/demo/export/timeline.ts`    | Prompt speed and cursor-following camera              |
| `src/components/demo/export/screen.tsx`     | Captured colors and glyphs                            |
| `src/components/demo/export/screen.css`     | Terminal geometry for export                          |
| `src/components/demo/export/frame.tsx`      | Frame renderer driven by the export clock             |
| `scripts/video/render.mjs`                  | Deterministic screenshots, H.264 encoding, and poster |
| `scripts/recording/record.py`               | Isolated provider-backed editing session              |
| `scripts/recording/export.py`               | Convert raw PTY evidence to reviewed cells            |

## Playback

The recording shows GPT-5.6 Sol fixing a JavaScript slug formatter and running
four passing tests. The prompt is typed into ulo's live inline composer and
submitted with Enter. The clip starts at normal scale, plays typing at 2× speed
with a 2× close-up, then returns to normal scale and speed after submission.
These edits are baked into the video.

The ulo migration updates the captured banner's product name and regenerates
the video and poster. The recorded task, replies, tool output, and timing
remain from the original session.

Click the picture or the white icon to pause, play, or replay. Dragging the
timeline seeks and resumes on release. Space and Enter toggle playback when the
picture or timeline is focused; timeline arrow keys seek by 100 ms. The picture
cannot be selected or dragged. Native video context-menu actions remain available.
Reduced motion disables autoplay. Playback pauses offscreen and in background tabs.

## Record a new session

Requirements: an authenticated installed ulo, Node.js, Python 3 with `pyte`, and
an ulo source checkout containing `scripts/term.py`.

```sh
python3 scripts/recording/record.py
python3 scripts/recording/export.py /printed/evidence/directory /path/to/ulo
```

The recorder creates a disposable home and project, temporarily copies local ulo
authentication with private permissions, and removes that copy afterward. It
uses the installed binary in inline mode with extensions disabled. Raw evidence
stays in the printed temporary directory, outside the repository.

The export retains each changed frame and caps idle pauses at 1.5 seconds.
Keystroke delays vary between characters, words, and sentences. Export fails
without the submitted task and passing test output. Inspect the cells and scan
for secrets before committing a new recording.

## Encode the video

Start the dev server, then run the renderer with a Chrome or Chromium executable.
FFmpeg must be on PATH. The optional final argument is the dev server origin.

```sh
node scripts/video/render.mjs /path/to/chrome http://localhost:3107
```

The renderer creates a temporary `/demo-export` route and an isolated headless
browser profile, then removes both on completion. Do not build or deploy while
it runs. It refuses to replace an existing export route. After a forced process
termination, remove that temporary route before building.

The output is a 2160 × 1352 H.264 MP4 at 30 fps with a fast-start header, plus a
JPEG poster. Each frame is captured in order and CSS camera transitions advance
with the export clock, so a slow capture does not drop characters or extend pauses.
Commit the MP4, poster, and reviewed source recording together.

```sh
npm test
npm run test:recording
```

Tests protect prompt speed, camera bounds, individual letters, and the pause
before Enter. Browser review covers video loading, click-to-pause, seeking,
replay, offscreen pausing, keyboard controls, and reduced motion.
