"""Capture the live diff UI against a local Git fixture, without a reachable model provider.

Run after cargo build: python3 scripts/verify-diff.py [binary] [output-directory].
The HTML files render captured terminal cells, not a separate UI mockup.
"""
import hashlib
import html
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from term import replay

ROOT = Path(__file__).resolve().parent.parent
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/debug/e"
OUTPUT = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(tempfile.mkdtemp(prefix="e-diff-frames-"))
OUTPUT.mkdir(parents=True, exist_ok=True)

BEFORE = """export function formatRelative(ts: number): string {
  const diff = Date.now() - ts;
  if (diff < 60000) return 'just now';
  if (diff < 3600000) return mins(diff) + 'm ago';
  if (diff < 86400000) return hours(diff) + 'h ago';
  if (diff < 604800000) return days(diff) + 'd ago';
  return new Date(ts).toLocaleDateString('en-US', {
    month: 'short', day: 'numeric',
  });
}
"""
AFTER = """const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;

""" + BEFORE.replace("604800000", "WEEK").replace("86400000", "DAY").replace("3600000", "HOUR").replace("60000", "MINUTE")


def write_html(screen, destination, light):
    """Render the captured character grid and its actual foreground/background colors."""
    foreground, background = ("#25262b", "#ffffff") if light else ("#dddde2", "#28282c")
    def color(value, fallback):
        return fallback if value == "default" else "#" + value
    rows = []
    for y in range(screen.lines):
        spans = []
        for x in range(screen.columns):
            cell = screen.buffer[y][x]
            fg, bg = color(cell.fg, foreground), color(cell.bg, background)
            if cell.reverse:
                fg, bg = bg, fg
            style = f"color:{fg};background:{bg};font-weight:{'bold' if cell.bold else 'normal'}"
            spans.append(f'<span style="{style}">{html.escape(cell.data)}</span>')
        rows.append("".join(spans))
    destination.write_text(f'<!doctype html><meta charset="utf-8"><title>{destination.stem}</title>'
        f'<style>body{{background:{background};margin:20px}}pre{{font:15px/1.5 Menlo,monospace;font-variant-ligatures:none}}</style>'
        '<pre>' + "\n".join(rows) + '</pre>')


with tempfile.TemporaryDirectory(prefix="e-diff-fixture-") as temporary:
    lab = Path(temporary)
    home, work = lab / "home", lab / "work"
    home.mkdir()
    work.mkdir()
    cwd = str(work.resolve())
    env = dict(os.environ, E_HOME=str(home), GIT_CONFIG_GLOBAL=os.devnull,
               GIT_CONFIG_NOSYSTEM="1", COLORTERM="truecolor")
    def git(*args):
        subprocess.run(["git", "-C", cwd, *args], env=env, check=True, capture_output=True)
    git("init", "-b", "main", "--template=")
    (work / "src/lib").mkdir(parents=True)
    (work / "src/lib/time.ts").write_text(BEFORE)
    (work / "README.md").write_text("# Utilities\n")
    git("add", ".")
    git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "base")
    (work / "src/lib/time.ts").write_text(AFTER)
    (work / "README.md").write_text("# Utilities\n\nRelative time formatting.\n")
    (work / "src/lib/time.test.ts").write_text("it('returns just now', () => {\n  expect(formatRelative(Date.now())).toBe('just now');\n});\n")
    (home / "models.json").write_text(json.dumps({"providers": {"mock": {
        "base_url": "http://localhost:9", "api": "openai-completions", "catalog": "none", "models": ["test"]}}}))
    (home / "auth.json").write_text('{"mock":{"key":"test"}}')
    (home / "trust.json").write_text(json.dumps({cwd: {"trusted": True}}))
    folder = home / "sessions" / ("sha256-" + hashlib.sha256(cwd.encode()).hexdigest())
    folder.mkdir(parents=True)
    messages = [
        {"role": "user", "content": "Replace the magic numbers with named time constants and add a test."},
        {"role": "assistant", "content": "Three small edits, nothing committed or staged.\n\n- `src/lib/time.ts`: replaced the millisecond values with named constants.\n- `README.md`: added the relative time utility.\n- `src/lib/time.test.ts`: added a test for recent timestamps.\n\nThe function keeps the same behavior."},
    ]
    calls = [
        ("edit", {"path": "scripts/ptycap.py", "old_string": "old", "new_string": "new"}, summary)
        for summary in ["+1 -1", "+2 -1", "+1 -0", "+3 -2"]
    ] + [("read", {"path": "scripts/example.py"}, "1 line"),
         ("edit", {"path": "scripts/ptycap.py", "old_string": "old", "new_string": "new"}, "+1 -1"),
         ("bash", {"command": "printf first\nprintf second"}, "exit 0")]
    messages.append({"role": "assistant", "content": "", "tool_calls": [
        {"id": str(i), "name": name, "arguments": json.dumps(arguments)}
        for i, (name, arguments, _) in enumerate(calls)]})
    messages.extend({"role": "tool", "tool_call_id": str(i), "content": "recorded result",
                     "tool_meta": {"outcome": "completed", "summary": summary}}
                    for i, (_, _, summary) in enumerate(calls))
    entries = [{"type": "session", "format_version": 1, "id": "diff-review", "cwd": cwd,
                "created": int(time.time() * 1000), "model": "mock/test"}]
    for i, message in enumerate(messages):
        entries.append({"type": "message", "id": str(i), "parent": str(i-1) if i else None, "message": message})
    (folder / "review.jsonl").write_text("\n".join(json.dumps(entry) for entry in entries) + "\n")

    def pointer(code, x, y, release=False):
        return f"\x1b[<{code};{x};{y}{'m' if release else 'M'}"

    def capture(name, light, cols, rows, steps):
        """Replay actual pointer/keyboard input and retain its final terminal cells."""
        (home / "settings.json").write_text(json.dumps({"theme": "light" if light else "dark", "diff_refresh_ms": 250}))
        raw = OUTPUT / f"{name}.raw"
        capture_env = dict(env, CAP_PROMPT="", CAP_EXIT="", CAP_WAIT_FOR="", CAP_STEPS=json.dumps(steps))
        subprocess.run([sys.executable, str(ROOT / "scripts/ptycap.py"), str(raw), str(cols), str(rows), "0", "5.2",
                        str(BINARY), "--continue", "--no-save", "--no-extensions", "--model", "mock/test"],
                       cwd=work, env=capture_env, check=True)
        screen = replay(raw, cols, rows)
        text = "\n".join(screen.display)
        raw.with_suffix(".txt").write_text(text)
        write_html(screen, raw.with_suffix(".html"), light)
        assert "Ctrl+D focus" not in text and "Enter attach" not in text, text
        assert "lines from diff" not in screen.display[-1], text
        print(f"captured {name}: {cols}x{rows}", flush=True)
        return screen

    cols, rows = 180, 34
    x = cols - cols * 40 // 100 + 8
    base = [[1.2, "/diff\r"], [2.2, pointer(0, x, 4) + pointer(0, x, 4, True)]]
    probe = capture("dark", False, cols, rows, base)
    text = "\n".join(probe.display)
    assert text.count("Edited scripts/ptycap.py") == 2, text
    assert "+7 / -4" in text, text
    assert "printf second" in text, text
    first = next(i + 1 for i, line in enumerate(probe.display) if "if (diff < MINUTE)" in line)
    last = next(i + 1 for i, line in enumerate(probe.display) if "if (diff < WEEK)" in line)
    selection = [3.3, pointer(0, x, first) + pointer(32, x, last) + pointer(0, x, last, True)]
    single = [3.3, pointer(0, x, first) + pointer(0, x, first, True)]
    captures = [
        ("selected", False, [selection]),
        ("light", True, [selection]),
        ("single", False, [single]),
        ("armed", False, [selection, [4.2, "\x7f"]]),
        ("deleted", False, [selection, [4.1, "\x7f"], [4.5, "\x7f"]]),
        ("typing", False, [selection, [4.2, " add comments"]]),
        ("multiline", False, [selection, [4.2, "\x1b[200~ add comments\nand keep the behavior\x1b[201~"]]),
        ("replaced", False, [single, [4.2, selection[1]]]),
        ("sent", False, [single, [4.0, " explain this selection\r"]]),
        ("closed", False, [[3.3, pointer(0, cols - 1, 1)]]),
    ]
    for name, light, extra in captures:
        screen = capture(name, light, cols, rows, base + extra)
        text = "\n".join(screen.display)
        if name not in ["closed"]:
            assert "files changed" in text, text
        if name not in ["single", "deleted", "sent", "closed"]:
            assert text.count("⧉ 4 lines from diff") == 1, text
        if name == "single":
            assert "⧉ 1 line from diff" in text, text
        if name in ["deleted", "sent"]:
            assert "⧉" not in text, text
        if name == "armed":
            assert any(cell.reverse for row in screen.buffer.values() for cell in row.values()), text
        if name == "typing":
            assert "⧉ 4 lines from diff add comments" in text, text
        if name == "multiline":
            assert "┃ and keep the behavior" in text, text
        if name == "sent":
            assert "explain this selection" in text, text
            assert "Selected lines from src/lib/time.ts:" in text, text
        if name == "closed":
            assert "files changed" not in text and "Three small edits" in text, text
    narrow = capture("narrow", False, 80, 28, [[1.2, "/diff\r"], [2.2, pointer(0, 8, 4)], [3.3, "still typing"]])
    assert "┃ still typing" in "\n".join(narrow.display)
print(f"Verified terminal frames: {OUTPUT}")
