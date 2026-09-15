#!/usr/bin/env python3
"""Export reviewed Markdown into a release asset consumed by the public changelog."""
import json
import re
import sys
from pathlib import Path

GROUPS = ('New features', 'Improvements', 'Fixes')


def parse(body):
    """Keep the fixed groups and paragraphs; reject ungrouped bullets and empty notes."""
    title, intro, groups = '', [], {name: [] for name in GROUPS}
    active = None
    for line in body.splitlines():
        line = line.strip()
        if not line or re.fullmatch(r'\d{4}-\d{2}-\d{2}', line) or re.match(r'^[A-Z][a-z]+ \d{1,2}, \d{4}$', line):
            continue
        if line.startswith('### '):
            heading = line[4:]
            if heading in GROUPS:
                active = heading
            elif not title:
                title = heading
            else:
                raise ValueError(f'Unexpected release heading: {heading}')
        elif line.startswith('- '):
            if active is None:
                raise ValueError('Release bullets need a fixed group')
            groups[active].append(line[2:])
        elif active and groups[active]:
            groups[active][-1] += ' ' + line
        else:
            intro.append(line)
    if not title or not any(groups.values()):
        raise ValueError('Release notes need a title and at least one grouped change')
    return {'title': title, 'intro': ' '.join(intro), 'groups': groups}


if __name__ == '__main__':
    result = parse(Path(sys.argv[1]).read_text())
    result.update(version=sys.argv[2].removeprefix('v'), commit=sys.argv[3])
    print(json.dumps(result, indent=2))
