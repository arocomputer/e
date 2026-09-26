"""Loopback provider shared by automated frame checks and interactive scenarios."""
import http.server
import json
import time

class Provider(http.server.BaseHTTPRequestHandler):
    """Emit paced completions, a wire error, or a truncated stream by prompt."""

    def log_message(self, *args):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.requests.append(request)
        prompt = next(m['content'] for m in reversed(request['messages']) if m['role'] == 'user')
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        if prompt == 'body-error':
            self.send_header('Content-Length', '500')
        self.end_headers()
        try:
            if prompt == 'scroll-chat':
                initial = ''.join(f'History row {i:02d}\n\n' for i in range(40))
                events = [{'choices': [{'delta': {'content': initial}}]}]
                events += [{'choices': [{'delta': {'content': f'Live row {i:03d}\n\n'}}]} for i in range(120)]
                events += [{'choices': [{'delta': {'content': 'SCROLL_CHAT_FINISHED'}}]}]
            elif prompt == 'visible-work':
                if any(message['role'] == 'tool' for message in request['messages']):
                    events = [{'choices': [{'delta': {'content': 'VISIBLE_WORK_FINISHED'}}]}]
                else:
                    calls = [{'index': i, 'id': f'visible-{i}', 'type': 'function', 'function': {
                        'name': 'bash', 'arguments': json.dumps({'command': f'printf "work-{i:02d}\\n"' + ('; exit 7' if i == 1 else '')})}}
                        for i in range(16)]
                    events = [
                        {'choices': [{'delta': {'reasoning_content': 'RETAINED_THINKING_DETAIL'}}]},
                        {'choices': [{'delta': {'tool_calls': calls}, 'finish_reason': 'tool_calls'}]},
                    ]
            elif prompt == 'diff-counts':
                if any(message['role'] == 'tool' for message in request['messages']):
                    events = [{'choices': [{'delta': {'content': 'DIFF_FINISHED'}}]}]
                else:
                    call = {'index': 0, 'id': 'edit-1', 'type': 'function', 'function': {
                        'name': 'edit', 'arguments': json.dumps({
                            'path': 'sample.txt', 'old_string': 'old line', 'new_string': 'new line\nextra line'})}}
                    events = [{'choices': [{'delta': {'tool_calls': [call]}, 'finish_reason': 'tool_calls'}]}]
            elif prompt in ('tool-tree', 'single-tool', 'heredoc-tool'):
                if any(message['role'] == 'tool' for message in request['messages']):
                    marker = {'single-tool': 'SINGLE_TOOL_FINISHED', 'heredoc-tool': 'HEREDOC_FINISHED'}.get(prompt, 'CONNECTED_TOOLS_FINISHED')
                    events = [{'choices': [{'delta': {'content': marker}}]}]
                else:
                    commands = [
                        "printf 'A long command summary that wraps without losing its arguments\\n'; for i in 1 2 3 4 5 6 7 8 9 10 11 12; do printf 'first command output row %s with a long suffix\\n' \"$i\"; sleep 0.2; done",
                        "printf 'Second concurrent command\\n'; sleep 0.8; printf 'SECOND_FINISHED\\n'",
                    ]
                    if prompt == 'single-tool':
                        commands = ["printf 'SINGLE_OUTPUT\\n'; sleep 1 # a single command with arguments long enough to wrap"]
                    if prompt == 'heredoc-tool':
                        commands = ["cat <<'E_LABEL_SCRIPT' >/dev/null\n" +
                                    'HEREDOC_BODY_ONLY ctrl+o to view\n' * 3 +
                                    "E_LABEL_SCRIPT\nprintf 'REVIEW_LINE_ONE\\nREVIEW_LINE_TWO\\nREVIEW_LINE_THREE\\nREVIEW_LINE_FOUR\\n'"]
                    calls = [{'index': i, 'id': f'tool-{i}', 'type': 'function',
                              'function': {'name': 'bash', 'arguments': json.dumps({'command': command})}}
                             for i, command in enumerate(commands)]
                    events = [{'choices': [{'delta': {'tool_calls': calls}, 'finish_reason': 'tool_calls'}]}]
            elif prompt in ('wire-error', 'wire-error-partial'):
                events = [{'error': {'message': 'BOUNTY upstream quota exhausted', 'type': 'insufficient_quota', 'code': 429}}]
                if prompt == 'wire-error-partial':
                    events.insert(0, {'choices': [{'delta': {'content': 'Incomplete answer accepted as success.'}}]})
            elif prompt in ('disconnect', 'body-error'):
                events = [{'choices': [{'delta': {'content': 'Partial answer before disconnect.'}}]}]
            else:
                text = '# Streaming audit\n\nUnicode: 界界 café 👩‍💻.\n\n'
                text += '| Name | Value |\n| --- | --- |\n| First | 123 |\n\n'
                text += '```python\nfor i in range(3):\n    print(i)\n```\n\n'
                text += ''.join(f'Line {i:02d}: paced streaming text.\n\n' for i in range(30))
                text += 'BOUNTY_STREAM_DONE\n'
                events = [{'choices': [{'delta': {'content': text[i:i + 12]}}]} for i in range(0, len(text), 12)]
            for event in events:
                self.wfile.write(('data: ' + json.dumps(event) + '\n\n').encode())
                self.wfile.flush()
                time.sleep(0.035)
            if prompt not in ('disconnect', 'body-error'):
                self.wfile.write(b'data: [DONE]\n\n')
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass



def configure(state, port):
    """Write dummy credentials and a loopback-only model catalog into an isolated home."""
    (state / 'models.json').write_text(json.dumps({'providers': {'mock': {
        'base_url': f'http://127.0.0.1:{port}', 'catalog': 'none',
        'api': 'openai-completions', 'models': ['audit']}}}))
    (state / 'auth.json').write_text('{"mock":{"key":"synthetic-test-key"}}')
    (state / 'auth.json').chmod(0o600)
