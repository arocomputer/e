"""Render published Markdown with public dependencies, without executing it."""
from html import escape
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def body(source):
    if not source.startswith('---\n'):
        raise ValueError('guide is missing front matter')
    metadata, separator, content = source[4:].partition('\n---\n')
    if not separator or not metadata.strip() or not content.strip():
        raise ValueError('guide has incomplete front matter or no body')
    return content


def main():
    from markdown_it import MarkdownIt
    renderer = MarkdownIt('commonmark', {'html': False}).enable(['table', 'strikethrough'])
    guides = sorted((ROOT / 'docs/guides').glob('*/*.md'))
    if not guides:
        raise ValueError('no published guides found')
    output = ROOT / 'target/site-guides'
    for guide in guides:
        content = body(guide.read_text())
        rendered = renderer.render(content)
        if not rendered.strip():
            raise ValueError(f'empty rendered guide: {guide}')
        relative = guide.relative_to(ROOT / 'docs/guides').with_suffix('.html')
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(f'<!doctype html><meta charset="utf-8"><title>{escape(guide.stem)}</title>\n{rendered}')
    print(f'Rendered {len(guides)} Markdown guides to {output}')


if __name__ == '__main__':
    main()
