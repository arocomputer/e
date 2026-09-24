"""Export reviewed PTY evidence as timed cells, preserving streamed text and tool output."""

import bisect
import json
from pathlib import Path
import re
import sys


def main():
    """Replay ulo's completed frames; keep composer typing and cap idle pauses at 1.5 seconds."""
    evidence = Path(sys.argv[1])
    sys.path.insert(0, str(Path(sys.argv[2]) / "scripts"))
    from term import replay

    samples = json.loads((evidence / "samples.json").read_text())
    offsets = [sample[1] for sample in samples]
    raw = (evidence / "session.raw").read_bytes()
    ends = [match.end() for match in re.finditer(re.escape(b"\x1b[?2026l"), raw)]
    frames = []
    index = 0

    def capture(screen):
        nonlocal index
        timestamp = samples[
            min(bisect.bisect_left(offsets, ends[index]), len(samples) - 1)
        ][0]
        index += 1
        rows = []
        for y in range(screen.lines):
            spans = []
            for x in range(screen.columns):
                cell = screen.buffer[y][x]
                style = {
                    "fg": cell.fg,
                    "bg": cell.bg,
                    "reverse": cell.reverse,
                    "bold": cell.bold,
                    "italic": cell.italics,
                }
                if spans and all(
                    spans[-1][key] == value for key, value in style.items()
                ):
                    spans[-1]["text"] += cell.data
                else:
                    spans.append({"text": cell.data, **style})
            rows.append(spans)
        cursor = {
            "x": screen.cursor.x,
            "y": screen.cursor.y,
            "hidden": screen.cursor.hidden,
        }
        if frames and frames[-1]["rows"] == rows and frames[-1]["cursor"] == cursor:
            return
        frames.append({"time": timestamp, "rows": rows, "cursor": cursor})

    replay(evidence / "session.raw", 100, 28, on_frame=capture)
    text = "\n".join(
        "".join(s["text"] for s in row) for f in frames for row in f["rows"]
    )
    assert "All four slug tests pass" in text, "Missing successful test output"
    assert "slugify.mjs" in text, "Missing project edit"
    assert "Fix slugify" in text, "Missing submitted prompt"
    input_started = json.loads((evidence / "input.json").read_text())["started"]
    first = max(
        0, bisect.bisect_right([frame["time"] for frame in frames], input_started) - 1
    )
    frames = frames[first:]
    frames[0]["time"] = input_started
    for i, frame in enumerate(frames):
        frame["duration"] = (
            min(1500, max(1, frames[i + 1]["time"] - frame["time"]))
            if i + 1 < len(frames)
            else 3000
        )
    for frame in frames:
        del frame["time"]
    target = Path(__file__).resolve().parents[2] / "src/components/demo/export/recording.json"
    target.write_text(
        json.dumps(frames, ensure_ascii=False, separators=(",", ":")) + "\n"
    )
    print(
        f"Exported {len(frames)} real frames, {sum(f['duration'] for f in frames) / 1000:.1f}s"
    )


if __name__ == "__main__":
    main()
