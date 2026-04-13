import importlib.util
import unittest
from pathlib import Path


def load_module():
    script_path = (
        Path(__file__).resolve().parent.parent
        / "scripts"
        / "transcribe_with_google_genai.py"
    )
    spec = importlib.util.spec_from_file_location("transcribe_with_google_genai", script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


transcribe = load_module()


class TranscribeWithGoogleGenAiTests(unittest.TestCase):
    def test_normalize_extractor_markdown_removes_blank_line_after_section_heading(self):
        input_text = (
            "# Hotel Folio\n\n"
            "## Stay Summary\n\n"
            "- Property Name: Marina Bay Grand Hotel\n"
            "- Guest Name: Olivia Park\n"
        )

        normalized = transcribe.normalize_extractor_markdown(input_text)

        self.assertEqual(
            normalized,
            "# Hotel Folio\n\n## Stay Summary\n- Property Name: Marina Bay Grand Hotel\n- Guest Name: Olivia Park",
        )

    def test_normalize_extractor_markdown_preserves_blank_before_next_heading(self):
        input_text = "# Hotel Folio\n\n## Stay Summary\n"

        normalized = transcribe.normalize_extractor_markdown(input_text)

        self.assertEqual(normalized, "# Hotel Folio\n\n## Stay Summary")

    def test_normalize_pages_applies_markdown_cleanup(self):
        payload = {
            "pages": [
                {
                    "page_number": 1,
                    "text": "## Nightly Charges\n\n- Date: 2025-04-21 | Description: Room",
                }
            ]
        }

        pages = transcribe.normalize_pages(payload)

        self.assertEqual(
            pages,
            [
                {
                    "page_number": 1,
                    "text": "## Nightly Charges\n- Date: 2025-04-21 | Description: Room",
                }
            ],
        )

    def test_normalize_extractor_markdown_merges_wrapped_pipe_rows(self):
        input_text = (
            "### charges\n"
            "* Date: 2025-04-24 | Description: Garden Superior Room | Room Rate: JPY 225.00 |\n"
            "* Taxes & Fees: JPY 40.50\n"
        )

        normalized = transcribe.normalize_extractor_markdown(input_text)

        self.assertEqual(
            normalized,
            "### charges\n* Date: 2025-04-24 | Description: Garden Superior Room | Room Rate: JPY 225.00 | Taxes & Fees: JPY 40.50",
        )


if __name__ == "__main__":
    unittest.main()
