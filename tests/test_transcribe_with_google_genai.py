import importlib.util
import io
import json
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
    @staticmethod
    def make_png_bytes() -> bytes:
        from PIL import Image

        image = Image.new("RGB", (8, 8), color=(220, 220, 220))
        buffer = io.BytesIO()
        image.save(buffer, format="PNG")
        return buffer.getvalue()

    def test_generate_content_with_retries_recovers_from_transient_failure(self):
        class FakeModels:
            def __init__(self):
                self.calls = 0

            def generate_content(self, **kwargs):
                self.calls += 1
                if self.calls == 1:
                    raise RuntimeError("connection reset by peer")
                return {"ok": True, "kwargs": kwargs}

        class FakeClient:
            def __init__(self):
                self.models = FakeModels()

        sleeps = []
        client = FakeClient()

        response = transcribe.generate_content_with_retries(
            client,
            model="gemini-3-flash-preview",
            contents=["prompt"],
            config={"temperature": 0},
            sleep_fn=sleeps.append,
        )

        self.assertEqual(client.models.calls, 2)
        self.assertEqual(sleeps, [1])
        self.assertEqual(response["ok"], True)

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
                    "dimensions": None,
                    "regions": [],
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
                    "dimensions": None,
                    "regions": [],
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

    def test_build_prompt_adds_table_focused_hinting(self):
        prompt = transcribe.build_prompt(
            "receipt.png", "image/png", pass_kind="table_focused"
        )
        self.assertIn("Prioritize preserving table rows", prompt)

    def test_build_grounding_prompt_targets_receipt_fields(self):
        prompt = transcribe.build_grounding_prompt(
            "receipt.png",
            "# Merchant Receipt\n- Merchant Name: BOOK TALK\n- Total Paid: MYR 80.90",
        )

        self.assertIn("merchant_name", prompt)
        self.assertIn("transaction_date", prompt)
        self.assertIn("total_paid", prompt)
        self.assertIn("normalized 0-1000 coordinates", prompt)

    def test_normalize_grounding_regions_converts_box_coordinates(self):
        normalized = transcribe.normalize_grounding_regions(
            {
                "regions": [
                    {
                        "region_id": "total_paid",
                        "kind": "value_candidate",
                        "text": "MYR 80.90",
                        "box_2d": [100, 200, 160, 520],
                    }
                ]
            }
        )

        self.assertEqual(
            normalized,
            [
                {
                    "region_id": "total_paid",
                    "kind": "value_candidate",
                    "text": "MYR 80.90",
                    "bbox": {
                        "left": 0.2,
                        "top": 0.1,
                        "width": 0.32,
                        "height": 0.06,
                    },
                }
            ],
        )

    def test_maybe_ground_key_receipt_fields_skips_non_image_inputs(self):
        pages = [{"page_number": 1, "text": "# Merchant Receipt", "dimensions": None, "regions": []}]

        grounded_pages, geometry_source, geometry_available, grounding_variant = (
            transcribe.maybe_ground_key_receipt_fields(
                object(),
                model="gemini-3-flash-preview",
                filename="receipt.pdf",
                file_bytes=b"%PDF-1.4 mock",
                mime_type="application/pdf",
                normalized_pages=pages,
            )
        )

        self.assertEqual(grounded_pages, pages)
        self.assertEqual(geometry_source, "none")
        self.assertFalse(geometry_available)
        self.assertIsNone(grounding_variant)

    def test_grounding_retry_variants_try_primary_then_unique_fallbacks(self):
        self.assertEqual(
            transcribe.grounding_retry_variants("original"),
            ["original", "contrast_boosted", "binarized", "grayscale"],
        )
        self.assertEqual(
            transcribe.grounding_retry_variants("binarized"),
            ["binarized", "contrast_boosted", "grayscale"],
        )

    def test_maybe_ground_key_receipt_fields_retries_fallback_variant(self):
        pages = [{"page_number": 1, "text": "# Merchant Receipt", "dimensions": None, "regions": []}]
        attempted_payloads = []
        source_png = self.make_png_bytes()

        def fake_generate(_, **kwargs):
            image_bytes, detected_mime = kwargs["contents"][1]
            attempted_payloads.append((image_bytes, detected_mime))
            if image_bytes == source_png:
                text = json.dumps({"regions": []})
            else:
                text = json.dumps(
                    {
                        "regions": [
                            {
                                "region_id": "total_paid",
                                "kind": "value_candidate",
                                "text": "MYR 9.00",
                                "box_2d": [100, 200, 160, 520],
                            }
                        ]
                    }
                )
            return type("Response", (), {"text": text})()

        def fake_preprocess(document_path, file_bytes, mime_type, variant):
            self.assertEqual(document_path, Path("receipt.png"))
            self.assertEqual(file_bytes, source_png)
            self.assertEqual(mime_type, "image/png")
            return f"{variant}-bytes".encode(), "image/png"

        grounded_pages, geometry_source, geometry_available, grounding_variant = (
            transcribe.maybe_ground_key_receipt_fields(
                object(),
                model="gemini-3-flash-preview",
                filename="receipt.png",
                file_bytes=source_png,
                mime_type="image/png",
                normalized_pages=pages,
                document_path=Path("receipt.png"),
                source_file_bytes=source_png,
                source_mime_type="image/png",
                preprocess_variant="original",
                generate_fn=fake_generate,
                preprocess_fn=fake_preprocess,
                part_factory=lambda data, detected_mime: (data, detected_mime),
            )
        )

        self.assertEqual(
            attempted_payloads,
            [(source_png, "image/png"), (b"contrast_boosted-bytes", "image/png")],
        )
        self.assertEqual(geometry_source, "gemini")
        self.assertTrue(geometry_available)
        self.assertEqual(grounding_variant, "contrast_boosted")
        self.assertEqual(grounded_pages[0]["regions"][0]["region_id"], "total_paid")

    def test_maybe_ground_key_receipt_fields_returns_none_when_all_variants_fail(self):
        pages = [{"page_number": 1, "text": "# Merchant Receipt", "dimensions": None, "regions": []}]
        source_png = self.make_png_bytes()

        def fake_generate(_, **kwargs):
            return type("Response", (), {"text": json.dumps({"regions": []})})()

        grounded_pages, geometry_source, geometry_available, grounding_variant = (
            transcribe.maybe_ground_key_receipt_fields(
                object(),
                model="gemini-3-flash-preview",
                filename="receipt.png",
                file_bytes=source_png,
                mime_type="image/png",
                normalized_pages=pages,
                document_path=Path("receipt.png"),
                source_file_bytes=source_png,
                source_mime_type="image/png",
                preprocess_variant="original",
                generate_fn=fake_generate,
                preprocess_fn=lambda *_args: (b"retry-bytes", "image/png"),
                part_factory=lambda data, detected_mime: (data, detected_mime),
            )
        )

        self.assertEqual(grounded_pages[0]["regions"], [])
        self.assertEqual(geometry_source, "none")
        self.assertFalse(geometry_available)
        self.assertIsNone(grounding_variant)

    def test_preprocess_document_bytes_binarized_renders_png(self):
        from PIL import Image

        image = Image.new("RGB", (8, 8), color=(220, 220, 220))
        image.putpixel((4, 4), (20, 20, 20))
        buffer = io.BytesIO()
        image.save(buffer, format="PNG")

        processed_bytes, processed_mime = transcribe.preprocess_document_bytes(
            Path("receipt.png"),
            buffer.getvalue(),
            "image/png",
            "binarized",
        )

        self.assertEqual(processed_mime, "image/png")
        self.assertGreater(len(processed_bytes), 0)
        self.assertNotEqual(processed_bytes, buffer.getvalue())

    def test_preprocess_document_bytes_leaves_pdf_bytes_unchanged(self):
        pdf_bytes = b"%PDF-1.4 mock"
        processed_bytes, processed_mime = transcribe.preprocess_document_bytes(
            Path("receipt.pdf"),
            pdf_bytes,
            "application/pdf",
            "contrast_boosted",
        )

        self.assertEqual(processed_bytes, pdf_bytes)
        self.assertEqual(processed_mime, "application/pdf")


if __name__ == "__main__":
    unittest.main()
