#!/usr/bin/env python3
"""Record SSE response bodies for offline provider regression tests; never persist request headers."""
import argparse
import json
import os
from pathlib import Path
import re
import urllib.request
import urllib.parse


def redact(body, secret=''):
    """Remove the supplied credential and named credential fields from response text."""
    if secret:
        body = body.replace(secret, '[redacted]')
    return re.sub(r'("(?:access_token|refresh_token|api_key|authorization|account_id)"\s*:\s*")[^"]*', r'\1[redacted]', body, flags=re.I)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    """Authenticated fixture requests must never forward credentials through redirects."""
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('Use the final provider endpoint; redirects are disabled')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('url')
    parser.add_argument('request', type=Path, help='Synthetic request JSON; recording sends a real request')
    parser.add_argument('output', type=Path)
    parser.add_argument('--key-env', help='Environment variable containing the API key')
    parser.add_argument('--auth-header', choices=['Authorization', 'x-api-key'], default='Authorization')
    args = parser.parse_args()
    url = urllib.parse.urlsplit(args.url)
    if url.scheme != 'https' and not (url.scheme == 'http' and url.hostname in ('127.0.0.1', 'localhost')):
        parser.error('Use HTTPS or a loopback fixture server')
    secret = os.environ[args.key_env] if args.key_env else ''
    headers = {'Content-Type': 'application/json', 'Accept': 'text/event-stream'}
    if secret:
        headers[args.auth_header] = ('Bearer ' if args.auth_header == 'Authorization' else '') + secret
    request = urllib.request.Request(args.url, data=args.request.read_bytes(), headers=headers)
    with urllib.request.build_opener(NoRedirect).open(request, timeout=60) as response:
        body = response.read().decode('utf-8')
    record = {'origin': 'recorded', 'responses': [redact(body, secret)]}
    # Exclusive creation prevents accidentally overwriting a committed regression fixture.
    with args.output.open('x') as output:
        json.dump(record, output, indent=2)
        output.write('\n')
    print('Recorded response only. Review text for private content before committing it.')


if __name__ == '__main__':
    main()
