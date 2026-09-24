import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { terminalPlayback } from "../../src/sites/ulo/components/demo/export/timeline.ts";

const frames = JSON.parse(
  readFileSync(
    new URL(
      "../../src/sites/ulo/components/demo/export/recording.json",
      import.meta.url,
    ),
  ),
);

test("only the live draft gets double speed and a close-up", () => {
  const playback = terminalPlayback(frames);
  const first = playback.findIndex((frame) => frame.typing);
  const submitted = playback.findIndex(
    (frame, index) => index > first && !frame.typing,
  );
  assert.equal(first, 1);
  assert.ok(submitted > first);
  assert.ok(playback.slice(first, submitted).every((frame) => frame.typing));
  assert.ok(playback.slice(submitted).every((frame) => !frame.typing));
  for (const [index, frame] of playback.entries()) {
    assert.equal(
      frame.duration,
      frames[index].duration / (frame.typing ? 2 : 1),
    );
    if (index)
      assert.equal(
        frame.start,
        playback[index - 1].start + playback[index - 1].duration,
      );
    assert.ok(frame.pan >= 0 && frame.pan <= 50);
    if (!frame.typing) assert.equal(frame.pan, 0);
  }
  assert.ok(playback.some((frame) => frame.pan === 50));
});
