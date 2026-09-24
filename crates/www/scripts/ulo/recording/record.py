"""Record a real provider-backed ulo editing session in a disposable project.

Uses the user's ulo authentication privately, with separate settings and workspace.
Raw PTY evidence stays outside the repository; only reviewed cell frames are published.
"""

import fcntl
import json
import os
from pathlib import Path
import pty
import random
import select
import shutil
import signal
import struct
import subprocess
import tempfile
import termios
import time


def typing_events(prompt):
    """Schedule individual keystrokes and a held final draft before Enter."""
    elapsed = 0.8
    rhythm = random.Random(7)
    for index, character in enumerate(prompt):
        yield elapsed, character.encode("utf-8")
        elapsed += rhythm.uniform(0.055, 0.12)
        if character == " ":
            elapsed += rhythm.uniform(0.025, 0.09)
        elif character in ".,!?:" and (
            index + 1 == len(prompt) or prompt[index + 1] == " "
        ):
            elapsed += rhythm.uniform(0.18, 0.32)
    yield elapsed + 0.65, b"\r"


def main():
    """Run the installed ulo, capture its output, and verify the resulting code before export."""
    root = Path(tempfile.mkdtemp(prefix="ulo-edit-recording-")).resolve()
    state = root / ".ulo"
    project = root / "slugify"
    state.mkdir(mode=0o700)
    project.mkdir()
    auth = state / "auth.json"
    shutil.copyfile(Path.home() / ".ulo/auth.json", auth)
    auth.chmod(0o600)
    (state / "settings.json").write_text(
        json.dumps({"auto_update": "off", "tui_mode": "inline"})
    )
    (state / "trust.json").write_text(json.dumps({str(project): {"trusted": True}}))
    (project / "slugify.mjs").write_text(
        'export function slugify(text) {\n  return text.toLowerCase().replaceAll(" ", "-");\n}\n'
    )
    (project / "slugify.test.mjs").write_text(
        'import assert from "node:assert/strict";\nimport { slugify } from "./slugify.mjs";\nassert.equal(slugify("Hello World"), "hello-world");\nassert.equal(slugify("  Café & Code!  "), "cafe-code");\nassert.equal(slugify("one   two---three"), "one-two-three");\nassert.equal(slugify("!!!"), "");\nconsole.log("All four slug tests pass.");\n'
    )
    prompt = "Fix slugify.mjs so the existing tests pass. Explain the bug, edit the implementation, and run node slugify.test.mjs. Do not modify tests. Keep your explanation concise. Only access files in this project."
    env = {
        "HOME": str(root),
        "ULO_HOME": str(state),
        "PATH": os.environ["PATH"],
        "TERM": "xterm-256color",
        "LANG": "en_US.UTF-8",
    }
    binary = shutil.which("ulo")
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(project)
        os.execve(
            binary,
            [
                binary,
                "--no-extensions",
                "--model",
                "openai-codex/gpt-5.6-sol",
                "--effort",
                "low",
            ],
            env,
        )
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 100, 0, 0))
    raw = bytearray()
    samples = []
    started = time.monotonic()
    answered = set()
    verified_at = None
    input_started = None
    pending = iter(typing_events(prompt))
    next_key = next(pending, None)
    key_due = None
    try:
        while time.monotonic() - started < 180:
            timeout = (
                0.1
                if key_due is None
                else min(0.1, max(0, key_due - (time.monotonic() - started)))
            )
            if select.select([fd], [], [], timeout)[0]:
                try:
                    chunk = os.read(fd, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                raw.extend(chunk)
                for query, response in [
                    (b"\x1b]11;?", b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
                    (b"\x1b[6n", b"\x1b[1;1R"),
                ]:
                    if query in raw and query not in answered:
                        os.write(fd, response)
                        answered.add(query)
                if b"\x1b[?2026l" in chunk:
                    samples.append(
                        (round((time.monotonic() - started) * 1000), len(raw))
                    )
            elapsed = time.monotonic() - started
            # Wait for the first painted composer and terminal probe responses.
            if input_started is None and samples and elapsed > 2:
                input_started = elapsed
                key_due = elapsed + next_key[0]
            if key_due is not None and elapsed >= key_due:
                os.write(fd, next_key[1])
                previous_time = next_key[0]
                next_key = next(pending, None)
                # Keep spacing after a late write instead of catching up in a burst.
                key_due = elapsed + next_key[0] - previous_time if next_key else None
            if (
                elapsed > 5
                and verified_at is None
                and (project / "slugify.mjs").read_text().count("replaceAll") == 0
            ):
                result = subprocess.run(
                    ["node", "slugify.test.mjs"], cwd=project, capture_output=True
                )
                if result.returncode == 0:
                    verified_at = elapsed
                    print(
                        "Edited implementation passes all four tests; recording final response.",
                        flush=True,
                    )
            if verified_at is not None and elapsed - verified_at > 20:
                break
    finally:
        (root / "input.json").write_text(
            json.dumps({"started": round((input_started or 0) * 1000)})
        )
        (root / "session.raw").write_bytes(raw)
        (root / "samples.json").write_text(json.dumps(samples))
        auth.unlink(missing_ok=True)
        os.kill(pid, signal.SIGTERM)
        os.close(fd)
        for _ in range(10):
            if os.waitpid(pid, os.WNOHANG)[0]:
                break
            time.sleep(0.1)
        else:
            os.kill(pid, signal.SIGKILL)
    (root / "session.raw").write_bytes(raw)
    (root / "samples.json").write_text(json.dumps(samples))
    result = subprocess.run(
        ["node", "slugify.test.mjs"], cwd=project, capture_output=True
    )
    print(
        f"Evidence: {root}; test exit: {result.returncode}; recorded frames: {len(samples)}",
        flush=True,
    )
    if result.returncode:
        raise SystemExit(
            "Recording did not produce a verified edit; not publishing it."
        )


if __name__ == "__main__":
    main()
