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
    def test_normalize_pages_accepts_top_level_markdown_fallback(self):
        pages = transcribe.normalize_pages(
            {
                "markdown": "## Receipt Summary\n\n- Merchant: Blue Bottle Coffee\n- Total: USD 12.40"
            }
        )

        self.assertEqual(
            pages,
            [
                {
                    "page_number": 1,
                    "text": "## Receipt Summary\n- Merchant: Blue Bottle Coffee\n- Total: USD 12.40",
                }
            ],
        )

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

    def test_normalize_extractor_markdown_collapses_repeated_blank_lines(self):
        input_text = (
            "# Receipt\n\n\n"
            "## Totals\n\n\n"
            "- Subtotal: USD 10.00\n\n\n"
            "- Tax: USD 0.80\n"
        )

        normalized = transcribe.normalize_extractor_markdown(input_text)

        self.assertEqual(
            normalized,
            "# Receipt\n\n## Totals\n\n- Subtotal: USD 10.00\n\n- Tax: USD 0.80",
        )

    def test_normalize_extractor_markdown_merges_split_receipt_label_and_value(self):
        input_text = (
            "# Merchant Receipt\n\n"
            "## Totals\n"
            "- Subtotal\n"
            "- SGD 28.00\n"
            "- Tax\n"
            "- SGD 2.52\n"
            "- Total\n"
            "- SGD 30.52\n"
        )

        normalized = transcribe.normalize_extractor_markdown(input_text)

        self.assertEqual(
            normalized,
            "# Merchant Receipt\n\n## Totals\n- Subtotal: SGD 28.00\n- Tax: SGD 2.52\n- Total: SGD 30.52",
        )

    def test_normalize_extractor_markdown_merges_split_receipt_item_amount(self):
        input_text = (
            "# Merchant Receipt\n\n"
            "## Line Items\n"
            "- Laksa Lunch\n"
            "- SGD 18.00\n"
            "- Iced Tea\n"
            "- SGD 6.00\n"
        )

        normalized = transcribe.normalize_extractor_markdown(input_text)

        self.assertEqual(
            normalized,
            "# Merchant Receipt\n\n## Line Items\n- Laksa Lunch | SGD 18.00\n- Iced Tea | SGD 6.00",
        )


if __name__ == "__main__":
    unittest.main()
