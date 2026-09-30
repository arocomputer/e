"""Small JSON-only GitHub CLI boundary for repository maintenance."""
import json
import subprocess


def request(path, method='GET', body=None):
    command = ['gh', 'api', '--method', method, path]
    if body is not None:
        command += ['--input', '-']
    result = subprocess.run(command, input=json.dumps(body) if body is not None else None,
                            text=True, capture_output=True, check=True)
    return json.loads(result.stdout) if result.stdout.strip() else None


def pages(path):
    result = subprocess.run(['gh', 'api', '--paginate', '--slurp', path],
                            text=True, capture_output=True, check=True)
    return json.loads(result.stdout)
