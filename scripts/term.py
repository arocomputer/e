"""Tolerant pyte screen: agents emit private-mode queries pyte does not model."""
import pyte
import re

class Screen(pyte.Screen):
    def report_device_status(self, *args, **kwargs):
        pass
    def write_process_input(self, *args, **kwargs):
        pass

def replay(path, cols, rows):
    data = open(path, "rb").read().decode("utf-8", "replace")
    # pyte does not model alternate-buffer ownership. Keep the chat screen
    # intact while full-height readers paint, then restore it on close.
    main = screen = Screen(cols, rows)
    for chunk in re.split(r"(\x1b\[\?1049[hl])", data):
        if chunk == "\x1b[?1049h":
            screen = Screen(cols, rows)
        elif chunk == "\x1b[?1049l":
            screen = main
        else:
            pyte.Stream(screen).feed(chunk)
    return screen
