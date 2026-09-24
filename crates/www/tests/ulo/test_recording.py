"""Protect the recorded prompt's keystroke and submission sequence."""

import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "record_demo", Path(__file__).resolve().parents[2] / "scripts/ulo/recording/record.py"
)
recorder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recorder)


class TypingTests(unittest.TestCase):
    """Input must reach the composer one character at a time before Enter."""

    def test_types_complete_prompt_before_submission(self):
        events = list(recorder.typing_events("Fix café."))
        self.assertEqual([key.decode() for _, key in events[:-1]], list("Fix café."))
        self.assertEqual(events[-1][1], b"\r")
        self.assertGreaterEqual(events[0][0], 0.5)
        self.assertTrue(all(b[0] > a[0] for a, b in zip(events, events[1:])))
        self.assertGreaterEqual(events[-1][0] - events[-2][0], 0.65)

    def test_published_recording_keeps_typing_before_response(self):
        frames = json.loads(
            (
                Path(__file__).resolve().parents[2] / "src/sites/ulo/components/demo/export/recording.json"
            ).read_text()
        )
        texts = [
            "\n".join("".join(cell["text"] for cell in row) for row in frame["rows"])
            for frame in frames
        ]
        self.assertNotIn("Fix slugify", texts[0])
        partial = next(
            i
            for i, text in enumerate(texts)
            if "Fix slugif" in text and "Fix slugify.mjs" not in text
        )
        submitted = next(i for i, text in enumerate(texts) if "Thinking" in text)
        self.assertLess(partial, submitted)
        self.assertIn(
            "Only access files in this project.",
            " ".join(texts[submitted - 1].replace("┃", "").split()),
        )
        self.assertIn("All four slug tests pass", texts[-1])

    def test_published_prompt_advances_one_letter_per_frame(self):
        frames = json.loads(
            (
                Path(__file__).resolve().parents[2] / "src/sites/ulo/components/demo/export/recording.json"
            ).read_text()
        )
        previous = ""
        for frame in frames:
            rows = ["".join(cell["text"] for cell in row) for row in frame["rows"]]
            if any("Thinking" in row for row in rows):
                break
            draft = "".join(
                "".join(row[2:].split()) for row in rows if row.startswith("┃ ")
            )
            if draft != previous:
                self.assertTrue(draft.startswith(previous))
                self.assertEqual(
                    len(draft) - len(previous), 1, "Export skipped a typed letter"
                )
            previous = draft
        self.assertTrue(previous.endswith("thisproject."))


if __name__ == "__main__":
    unittest.main()
