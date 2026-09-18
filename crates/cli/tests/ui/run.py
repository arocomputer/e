#!/usr/bin/env python3
"""Check real terminal frames against a loopback streaming provider.

Run through `./x ui`, which builds the binary and provides pyte:
  ./x ui --out /tmp/e-ui
Artifacts contain only generated prompts and a dummy credential. Each scenario
gets its own HOME, E_HOME, workspace, raw PTY capture, and rendered frames.
"""
import argparse
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import select
import signal
import sys
import tempfile
import struct
import threading
import time
import termios

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 'scripts'))

try:
    from term import replay
    from checks import CHECKS, verify
except ModuleNotFoundError as error:
    if error.name not in ('pyte', 'wcwidth'):
        raise
    raise SystemExit('UI checks need the packages in crates/cli/tests/ui/requirements.txt; see crates/cli/tests/ui/README.md') from error

if not __debug__:
    raise SystemExit('UI checks require Python assertions; run without -O or PYTHONOPTIMIZE')


from provider import Provider, configure


# Each step waits, then sends keys or resizes. Frames are taken before the
# next step, so transient streaming states survive alongside the final view.
SCENARIOS = {
    'scroll-chat': [(0.3, 'scroll-chat\r'), (0.3, 'draft stays'), (0.3, '\x1b[<64;3;10M' * 6), (1, ''), (0.3, (72, 18)), (0.3, '\x0f'), (0.3, '\x1b'), (3, '\x1b[F'), (0.5, '')],
    'scroll-fullscreen': [(0.3, 'scroll-chat\r'), (0.3, 'draft stays'), (0.3, '\x1b[5~'), (1, ''), (0.3, (72, 18)), (0.3, '\x1b[<65;3;10M'), (3, '\x1b[F'), (0.5, '')],
    'visible-work': [(0.3, 'visible-work\r'), (2, '\x0f'), (0.3, '\x1b[H'), (0.3, '\x1b'), (0.3, '/settings\r'), (0.3, '\x1b[B\x1b[B\x1b[C'), (0.3, '\x1b'), (0.3, '')],
    'single-tool': [(0.8, (44, 30)), (0.3, 'single-tool\r'), (0.5, ''), (2, ''), (0.3, (140, 30)), (0.3, (44, 30)), (0.3, '')],
    'heredoc-tool': [(0.8, 'heredoc-tool\r'), (2, '\x0f'), (0.5, '\x1b[C'), (0.5, '\x1b'), (0.5, '')],
    'tui-mode': [(0.8, '/settings\r'), (0.4, '\x1b[B\x1b[C'), (0.4, '\x1b'), (0.4, '/settings\r'), (0.4, '\x1b[B\x1b[D'), (0.4, '\x1b'), (0.4, '')],
    'diff-counts': [(0.8, 'diff-counts\r'), (2, '')],
    'tool-tree': [(0.8, 'tool-tree\r'), (0.3, 'draft while tools run'), (0.5, (44, 18)), (0.6, ''), (2, ''), (1, '')],
    'shell-composer': [(0.8, '!'), (0.3, "printf 'A command that wraps across the composer'"), (0.4, (36, 14)), (0.4, '\x01\x1b[3~'), (0.4, '')],
    'stream': [(0.8, 'stream\r'), (1, (72, 18)), (1, ''), (5, '')],
    'unfocused-stream': [(0.8, 'stream\r'), (0.5, '\x1b[O'), (4, '\x1b[I'), (0.5, '')],
    'wire-error': [(0.8, 'wire-error\r'), (2, '')],
    'wire-error-partial': [(0.8, 'wire-error-partial\r'), (2, '')],
    'path-control': [(0.8, '')],
    'paste-control': [(0.8, '\x1b[200~hello\x1b]0;BOUNTY_INJECTED\x07world\x1b[201~'), (0.5, '')],
    'disconnect': [(0.8, 'disconnect\r'), (2, '')],
    'body-error': [(0.8, 'body-error\r'), (2, '')],
    'cancel': [(0.8, 'stream\r'), (1, '\x1b'), (1, '')],
    'queue': [(0.8, 'stream\r'), (0.5, 'second prompt\r'), (0.5, '\x1b'), (6, '')],
    'plain-exit': [(0.8, '\x03\x03'), (0.5, '')],
    'trust-exit': [(0.8, '\x03\x03'), (0.5, '')],
    'settings-exit': [(0.8, '/settings\r'), (0.5, '\x03\x03'), (0.5, '')],
    'login-exit': [(0.8, '/login\r'), (0.5, '\x03\x03'), (0.5, '')],
    'hidden-paste': [(0.8, '/settings\r'), (0.5, '\x1b[200~UNSEEN_DRAFT\x1b[201~'), (0.5, '\x1b'), (0.5, '')],
    'crlf-paste': [(0.8, '\x1b[200~first\r\nsecond\x1b[201~'), (0.5, '')],
    'narrow-trust': [(0.8, (32, 10)), (0.5, '\x1b[6~'), (0.5, '\x1b[6~'), (0.5, '\x1b[B'), (0.5, '\x1b[5~'), (0.5, '')],
    'unicode-cursor': [(0.8, '\x1b[200~abcd\n界界\x1b[201~'), (0.5, '\x1b[A'), (0.5, '')],
}


# Bounded readiness checks supplement the pacing used to exercise live frames.
WAIT_FOR = {
    ('scroll-chat', 8): b'SCROLL_CHAT_FINISHED',
    ('scroll-fullscreen', 7): b'SCROLL_CHAT_FINISHED',
    ('visible-work', 1): b'VISIBLE_WORK_FINISHED',
    ('heredoc-tool', 1): b'HEREDOC_FINISHED',
    ('single-tool', 2): b'Running',
    ('single-tool', 3): b'SINGLE_TOOL_FINISHED',
    ('tool-tree', 1): b'Running',
    ('tool-tree', 5): b'CONNECTED_TOOLS_FINISHED',
    ('diff-counts', 1): b'DIFF_FINISHED',
    ('body-error', 1): b'Provider response interrupted.',
    ('shell-composer', 2): b'printf',
}


def capture(name, steps, out, port):
    """Run bounded PTY input steps, retaining every pre-action frame."""
    directory = out / name
    directory.mkdir(parents=True, exist_ok=False)
    home = directory / 'home'
    state = home / '.e'
    workspace = directory / ('workspace\x1b]0;BOUNTY_INJECTED\x07' if name == 'path-control' else 'workspace')
    state.mkdir(parents=True)
    workspace.mkdir()
    if name == 'diff-counts':
        (workspace / 'sample.txt').write_text('old line\n')
    configure(state, port)
    settings = {'auto_update': 'off'}
    if name in ('tool-tree', 'scroll-fullscreen', 'visible-work'):
        settings['tui_mode'] = 'fullscreen'
    (state / 'settings.json').write_text(json.dumps(settings))
    if name not in ('trust-exit', 'narrow-trust', 'path-control'):
        (state / 'trust.json').write_text(json.dumps({str(workspace): {'trusted': True}}))
    env = {'HOME': str(home), 'E_HOME': str(state), 'PATH': '/usr/bin:/bin',
           'TERM': 'xterm-256color', 'LANG': 'en_US.UTF-8'}
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(workspace)
        args = ['e', '--no-save', '--no-extensions', '--model', 'mock/audit']
        if name not in ('tool-tree', 'single-tool', 'heredoc-tool', 'diff-counts', 'visible-work'):
            args.append('--no-tools')
        os.execve(str(ROOT / 'target/debug/e'), args, env)
    raw = bytearray()
    sizes = []
    answered = set()
    alive = True
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 30, 100, 0, 0))

    def pump(seconds):
        """Drain output and answer only the two startup terminal probes."""
        nonlocal alive
        deadline = time.monotonic() + seconds
        while alive and time.monotonic() < deadline:
            if not select.select([fd], [], [], 0.02)[0]:
                continue
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                alive = False
                break
            if not chunk:
                alive = False
                break
            raw.extend(chunk)
            for query, response in [(b'\x1b]11;?', b'\x1b]11;rgb:0000/0000/0000\x1b\\'),
                                    (b'\x1b[6n', b'\x1b[1;1R')]:
                if query not in answered and query in raw:
                    os.write(fd, response)
                    answered.add(query)

    try:
        ready_until = time.monotonic() + 10
        while alive and b'\x1b[?2026l' not in raw and time.monotonic() < ready_until:
            pump(0.05)
        if b'\x1b[?2026l' not in raw:
            raise AssertionError('application did not paint its initial frame within 10s')
        for index, (wait, action) in enumerate(steps):
            pump(wait)
            marker = WAIT_FOR.get((name, index))
            if marker:
                until = time.monotonic() + 10
                while alive and marker not in raw and time.monotonic() < until:
                    pump(0.05)
                assert marker in raw, f'timed out waiting for {marker!r}'
            path = directory / f'{index:02d}.raw'
            path.write_bytes(raw)
            Path(str(path) + '.sizes.json').write_text(json.dumps(sizes))
            screen = replay(path, 100, 30)
            path.with_suffix('.txt').write_text('\n'.join(screen.display) + '\n')
            if not alive:
                break
            if isinstance(action, tuple):
                cols, rows = action
                sizes.append({'offset': len(raw), 'cols': cols, 'rows': rows})
                fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
            elif action:
                os.write(fd, action.encode())
        (directory / 'result.json').write_text(json.dumps({
            'alive_after_steps': alive, 'steps': steps, 'terminal_title': screen.title,
            'injected_osc_count': raw.count(b'\x1b]0;BOUNTY_INJECTED\x07'),
        }, indent=2))
        return directory
    finally:
        # Preserve the failing session too, before cleanup changes the screen.
        path = directory / 'session.raw'
        path.write_bytes(raw)
        Path(str(path) + '.sizes.json').write_text(json.dumps(sizes))
        path.with_suffix('.txt').write_text('\n'.join(replay(path, 100, 30).display) + '\n')
        reaped = False
        for sig in (signal.SIGTERM, signal.SIGKILL):
            if os.waitpid(pid, os.WNOHANG)[0]:
                reaped = True
                break
            try:
                os.kill(pid, sig)
            except ProcessLookupError:
                pass
            pump(0.3)
        os.close(fd)
        if not reaped:
            os.waitpid(pid, 0)


def main():
    """Start one loopback server and isolate each requested scenario."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, help='fresh artifact directory; defaults to a temporary directory')
    parser.add_argument('--record-only', action='store_true', help='capture exploratory scenarios without claiming a test pass')
    parser.add_argument('scenarios', nargs='*')
    args = parser.parse_args()
    names = args.scenarios or list(SCENARIOS if args.record_only else CHECKS)
    for name in names:
        if name not in SCENARIOS:
            parser.error(f'unknown scenario {name}; choose from {", ".join(SCENARIOS)}')
        if not args.record_only and name not in CHECKS:
            parser.error(f'{name} is exploratory; use --record-only')
    if args.out:
        out = args.out.resolve()
        try:
            out.mkdir(parents=True, exist_ok=False)
        except FileExistsError:
            parser.error(f'artifact directory already exists: {out}; choose a fresh path')
    else:
        # Match the canonical cwd used by directory trust, including macOS /private/var.
        out = Path(tempfile.mkdtemp(prefix='e-ui-')).resolve()
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    server.requests = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    failures = []
    print(f'UI artifacts: {out}', flush=True)
    try:
        for name in names:
            try:
                directory = capture(name, SCENARIOS[name], out, server.server_port)
                if not args.record_only:
                    verify(name, directory)
                print(f'{name}: {"recorded" if args.record_only else "passed"}', flush=True)
            except (AssertionError, OSError) as error:
                failures.append(name)
                print(f'{name}: FAILED: {error}', file=sys.stderr, flush=True)
    finally:
        server.shutdown()
        server.server_close()
        (out / 'requests.json').write_text(json.dumps(server.requests, indent=2))
    return 1 if failures else 0


if __name__ == '__main__':
    raise SystemExit(main())
