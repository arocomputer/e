"""Reject incomplete published sources before rendering."""
import unittest

from render_guides import body


class GuideTests(unittest.TestCase):
    def test_extracts_body_without_treating_metadata_as_markdown(self):
        self.assertEqual(body('---\ntitle: Example\n---\n\n# Guide\n'), '\n# Guide\n')

    def test_rejects_incomplete_sources(self):
        for source in ('# Guide', '---\ntitle: Example\n# Guide', '---\n\n---\n# Guide', '---\ntitle: Example\n---\n'):
            with self.subTest(source=source), self.assertRaises(ValueError):
                body(source)
