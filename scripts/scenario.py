#!/usr/bin/env python3
"""Launch an isolated terminal against repeatable local responses, without provider credentials."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import http.server

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('scenario', choices=['streaming', 'long-output', 'tools', 'cancellation', 'resume'])
    args = parser.parse_args()
    # Reuse the UI provider without loading the optional frame-checking dependencies.
    spec = importlib.util.spec_from_file_location('scenario_provider', ROOT / 'tests/ui/provider.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), module.Provider)
    server.requests = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='e-scenario-') as tmp:
        root = Path(tmp)
        state, project = root / 'state', root / 'project'
        state.mkdir(mode=0o700)
        project.mkdir()
        (state / 'settings.json').write_text(json.dumps({'auto_update': 'off'}))
        # Same provider/auth layout as the automated frame scenarios.
        module.configure(state, server.server_port)
        env = {'PATH': os.environ.get('PATH', ''), 'HOME': tmp, 'E_HOME': str(state),
               'TERM': os.environ.get('TERM', 'xterm-256color'), 'LANG': 'en_US.UTF-8'}
        prompt = 'single-tool' if args.scenario == 'tools' else 'stream'
        print('Local fixture only. Try Ctrl+C during streaming, Ctrl+O for tool output, or /resume.')
        try:
            subprocess.run([str(ROOT / 'target/debug/e'), '--no-extensions', '--model', 'mock/audit', *([] if args.scenario == 'tools' else ['--no-tools']), prompt], cwd=project, env=env, check=False)
            if args.scenario == 'resume':
                subprocess.run([str(ROOT / 'target/debug/e'), '--no-extensions', '--no-tools', '--model', 'mock/audit', '--continue'], cwd=project, env=env, check=False)
        finally:
            server.shutdown()


if __name__ == '__main__':
    main()
