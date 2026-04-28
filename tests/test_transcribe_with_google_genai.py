import importlib.util
import io
import json
import unittest
from pathlib import Path
from unittest import mock


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

    @staticmethod
    def make_jpeg_bytes(size=(8, 8)) -> bytes:
        from PIL import Image

        image = Image.new("RGB", size, color=(220, 220, 220))
        buffer = io.BytesIO()
        image.save(buffer, format="JPEG")
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

    def test_normalize_pages_accepts_bare_list_payload(self):
        payload = [
            {"page_number": 1, "text": "## Receipt\n- Total: USD 25.00"},
            {"page_number": 2, "text": "## Page 2\n- Tax: USD 2.50"},
        ]

        pages = transcribe.normalize_pages(payload)

        self.assertEqual(len(pages), 2)
        self.assertEqual(pages[0]["page_number"], 1)
        self.assertEqual(pages[0]["text"], "## Receipt\n- Total: USD 25.00")
        self.assertEqual(pages[1]["page_number"], 2)

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

    def test_build_receipt_localization_prompt_targets_outer_boundary(self):
        prompt = transcribe.build_receipt_localization_prompt("receipt.png")

        self.assertIn("outer boundary", prompt)
        self.assertIn("Exclude the desk, background", prompt)
        self.assertIn("box_2d", prompt)

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

    def test_normalize_localization_bbox_converts_box_coordinates(self):
        normalized = transcribe.normalize_localization_bbox(
            {"box_2d": [100, 200, 900, 800]}
        )

        self.assertEqual(
            normalized,
            {
                "left": 0.2,
                "top": 0.1,
                "right": 0.8,
                "bottom": 0.9,
            },
        )

    def test_detect_mime_type_prefers_heif_signature_over_extension(self):
        heif_bytes = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00"

        detected = transcribe.detect_mime_type(Path("receipt.png"), heif_bytes)

        self.assertEqual(detected, "image/heic")

    def test_preprocess_document_bytes_original_normalizes_image_uploads(self):
        jpeg_bytes = self.make_jpeg_bytes()

        processed_bytes, processed_mime = transcribe.preprocess_document_bytes(
            Path("receipt.jpg"),
            jpeg_bytes,
            "image/jpeg",
            "original",
        )

        self.assertEqual(processed_mime, "image/png")
        self.assertNotEqual(processed_bytes, jpeg_bytes)

    def test_canonicalize_document_bytes_for_ocr_surfaces_decode_errors_cleanly(self):
        with self.assertRaises(SystemExit) as context:
            transcribe.canonicalize_document_bytes_for_ocr(
                Path("receipt.png"),
                b"not-a-real-image",
                "image/png",
            )

        self.assertIn("could not decode image upload", str(context.exception))

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

    def test_maybe_localize_receipt_content_skips_small_receipts(self):
        small_png = self.make_png_bytes()

        localized_bytes, localized_mime, applied, bbox = (
            transcribe.maybe_localize_receipt_content(
                object(),
                model="gemini-3-flash-preview",
                filename="receipt.png",
                document_path=Path("receipt.png"),
                file_bytes=small_png,
                mime_type="image/png",
            )
        )

        self.assertEqual(localized_bytes, small_png)
        self.assertEqual(localized_mime, "image/png")
        self.assertFalse(applied)
        self.assertIsNone(bbox)

    def test_maybe_localize_receipt_content_crops_large_images_when_bbox_found(self):
        large_jpeg = self.make_jpeg_bytes(size=(3000, 2000))
        attempted = []

        def fake_generate(_client, **kwargs):
            attempted.append(kwargs["contents"][0])
            return type(
                "Response",
                (),
                {"text": json.dumps({"box_2d": [50, 100, 950, 700]})},
            )()

        localized_bytes, localized_mime, applied, bbox = (
            transcribe.maybe_localize_receipt_content(
                object(),
                model="gemini-3-flash-preview",
                filename="receipt.jpg",
                document_path=Path("receipt.jpg"),
                file_bytes=large_jpeg,
                mime_type="image/jpeg",
                generate_fn=fake_generate,
                part_factory=lambda data, detected_mime: (data, detected_mime),
            )
        )

        self.assertEqual(localized_mime, "image/png")
        self.assertTrue(applied)
        self.assertEqual(bbox["left"], 0.1)
        self.assertEqual(len(attempted), 1)
        self.assertNotEqual(localized_bytes, large_jpeg)

    def test_maybe_localize_receipt_content_falls_back_when_localization_fails(self):
        large_jpeg = self.make_jpeg_bytes(size=(3000, 2000))

        localized_bytes, localized_mime, applied, bbox = (
            transcribe.maybe_localize_receipt_content(
                object(),
                model="gemini-3-flash-preview",
                filename="receipt.jpg",
                document_path=Path("receipt.jpg"),
                file_bytes=large_jpeg,
                mime_type="image/jpeg",
                generate_fn=lambda *_args, **_kwargs: (_ for _ in ()).throw(
                    RuntimeError("localization failed")
                ),
                part_factory=lambda data, detected_mime: (data, detected_mime),
            )
        )

        self.assertEqual(localized_bytes, large_jpeg)
        self.assertEqual(localized_mime, "image/jpeg")
        self.assertFalse(applied)
        self.assertIsNone(bbox)

    def test_grounding_retry_variants_try_primary_then_unique_fallbacks(self):
        self.assertEqual(
            transcribe.grounding_retry_variants("original"),
            ["original", "contrast_boosted", "binarized", "grayscale"],
        )
        self.assertEqual(
            transcribe.grounding_retry_variants("binarized"),
            ["binarized", "contrast_boosted", "grayscale"],
        )

    def test_ocr_retry_variants_only_use_primary_for_non_images(self):
        self.assertEqual(
            transcribe.ocr_retry_variants("original", "application/pdf"),
            ["original"],
        )

    def test_ocr_retry_variants_try_primary_then_unique_fallbacks_for_images(self):
        self.assertEqual(
            transcribe.ocr_retry_variants("original", "image/png"),
            ["original", "contrast_boosted", "binarized", "grayscale"],
        )
        self.assertEqual(
            transcribe.ocr_retry_variants("binarized", "image/png"),
            ["binarized", "contrast_boosted", "grayscale"],
        )

    def test_attempt_transcription_pages_retries_empty_response_on_same_variant(self):
        source_png = self.make_png_bytes()
        call_count = 0

        def fake_generate(_, **kwargs):
            nonlocal call_count
            call_count += 1
            image_part = kwargs["contents"][1]
            self.assertEqual(image_part, (source_png, "image/png"))
            if call_count < 3:
                return type("Response", (), {"text": ""})()
            return type(
                "Response",
                (),
                {
                    "text": json.dumps(
                        {
                            "pages": [
                                {
                                    "page_number": 1,
                                    "text": "# Merchant Receipt\n- Total: USD 12.40",
                                }
                            ]
                        }
                    )
                },
            )()

        pages = transcribe.attempt_transcription_pages(
            object(),
            model="gemini-3-flash-preview",
            prompt=transcribe.build_prompt("receipt.png", "image/png", "primary"),
            file_bytes=source_png,
            mime_type="image/png",
            generate_fn=fake_generate,
            part_factory=lambda data, detected_mime: (data, detected_mime),
        )

        self.assertEqual(call_count, 3)
        self.assertEqual(pages[0]["text"], "# Merchant Receipt\n- Total: USD 12.40")

    def test_transcribe_pages_with_fallbacks_retries_after_empty_text_response(self):
        source_png = self.make_png_bytes()
        attempted_payloads = []
        per_variant_calls = {}

        def fake_generate(_, **kwargs):
            prompt, image_part = kwargs["contents"]
            self.assertIn("Transcribe this financial document", prompt)
            attempted_payloads.append(image_part)
            image_bytes, _mime = image_part
            per_variant_calls[image_bytes] = per_variant_calls.get(image_bytes, 0) + 1
            if image_bytes == source_png:
                return type("Response", (), {"text": ""})()
            return type(
                "Response",
                (),
                {
                    "text": json.dumps(
                        {
                            "pages": [
                                {
                                    "page_number": 1,
                                    "text": "# Merchant Receipt\n- Total: USD 12.40",
                                }
                            ]
                        }
                    )
                },
            )()

        def fake_preprocess(document_path, file_bytes, mime_type, variant):
            self.assertEqual(document_path, Path("receipt.png"))
            self.assertEqual(file_bytes, source_png)
            self.assertEqual(mime_type, "image/png")
            return f"{variant}-bytes".encode(), "image/png"

        pages, variant, file_bytes, mime_type = transcribe.transcribe_pages_with_fallbacks(
            object(),
            model="gemini-3-flash-preview",
            prompt=transcribe.build_prompt("receipt.png", "image/png", "primary"),
            markdown_fallback_prompt=transcribe.build_markdown_fallback_prompt(
                "receipt.png", "image/png", "primary"
            ),
            document_path=Path("receipt.png"),
            source_file_bytes=source_png,
            source_mime_type="image/png",
            primary_file_bytes=source_png,
            primary_mime_type="image/png",
            preprocess_variant="original",
            generate_fn=fake_generate,
            preprocess_fn=fake_preprocess,
            part_factory=lambda data, detected_mime: (data, detected_mime),
        )

        self.assertEqual(
            attempted_payloads,
            [(source_png, "image/png")] * transcribe.EMPTY_TRANSCRIPTION_RESPONSE_RETRIES
            + [(b"contrast_boosted-bytes", "image/png")],
        )
        self.assertEqual(
            per_variant_calls[source_png],
            transcribe.EMPTY_TRANSCRIPTION_RESPONSE_RETRIES,
        )
        self.assertEqual(variant, "contrast_boosted")
        self.assertEqual(file_bytes, b"contrast_boosted-bytes")
        self.assertEqual(mime_type, "image/png")
        self.assertEqual(pages[0]["text"], "# Merchant Receipt\n- Total: USD 12.40")

    def test_transcribe_pages_with_fallbacks_raises_after_all_variants_fail(self):
        source_png = self.make_png_bytes()

        with self.assertRaises(SystemExit) as context:
            transcribe.transcribe_pages_with_fallbacks(
                object(),
                model="gemini-3-flash-preview",
                prompt=transcribe.build_prompt("receipt.png", "image/png", "primary"),
                markdown_fallback_prompt=transcribe.build_markdown_fallback_prompt(
                    "receipt.png", "image/png", "primary"
                ),
                document_path=Path("receipt.png"),
                source_file_bytes=source_png,
                source_mime_type="image/png",
                primary_file_bytes=source_png,
                primary_mime_type="image/png",
                preprocess_variant="original",
                generate_fn=lambda *_args, **_kwargs: type("Response", (), {"text": ""})(),
                preprocess_fn=lambda *_args: (b"retry-bytes", "image/png"),
                part_factory=lambda data, detected_mime: (data, detected_mime),
            )

        self.assertIn("Gemini response did not contain text", str(context.exception))

    def test_transcribe_pages_with_fallbacks_retries_full_variant_cycle_once_more(self):
        source_png = self.make_png_bytes()
        attempted_payloads = []
        attempt_variants = transcribe.ocr_retry_variants("original", "image/png")
        calls_per_variant = transcribe.EMPTY_TRANSCRIPTION_RESPONSE_RETRIES
        first_cycle_calls = len(attempt_variants) * calls_per_variant

        def fake_generate(_, **kwargs):
            image_part = kwargs["contents"][1]
            attempted_payloads.append(image_part)
            if len(attempted_payloads) <= first_cycle_calls:
                return type("Response", (), {"text": ""})()
            return type(
                "Response",
                (),
                {
                    "text": json.dumps(
                        {
                            "pages": [
                                {
                                    "page_number": 1,
                                    "text": "# Merchant Receipt\n- Total: USD 12.40",
                                }
                            ]
                        }
                    )
                },
            )()

        def fake_preprocess(document_path, file_bytes, mime_type, variant):
            self.assertEqual(document_path, Path("receipt.png"))
            self.assertEqual(file_bytes, source_png)
            self.assertEqual(mime_type, "image/png")
            return f"{variant}-bytes".encode(), "image/png"

        pages, variant, file_bytes, mime_type = transcribe.transcribe_pages_with_fallbacks(
            object(),
            model="gemini-3-flash-preview",
            prompt=transcribe.build_prompt("receipt.png", "image/png", "primary"),
            markdown_fallback_prompt=transcribe.build_markdown_fallback_prompt(
                "receipt.png", "image/png", "primary"
            ),
            document_path=Path("receipt.png"),
            source_file_bytes=source_png,
            source_mime_type="image/png",
            primary_file_bytes=source_png,
            primary_mime_type="image/png",
            preprocess_variant="original",
            generate_fn=fake_generate,
            preprocess_fn=fake_preprocess,
            part_factory=lambda data, detected_mime: (data, detected_mime),
        )

        self.assertEqual(
            attempted_payloads[:calls_per_variant],
            [(source_png, "image/png")] * calls_per_variant,
        )
        self.assertEqual(
            attempted_payloads[calls_per_variant : calls_per_variant * 2],
            [(b"contrast_boosted-bytes", "image/png")] * calls_per_variant,
        )
        self.assertEqual(
            attempted_payloads[calls_per_variant * 2 : calls_per_variant * 3],
            [(b"binarized-bytes", "image/png")] * calls_per_variant,
        )
        self.assertEqual(
            attempted_payloads[calls_per_variant * 3 : first_cycle_calls],
            [(b"grayscale-bytes", "image/png")] * calls_per_variant,
        )
        self.assertEqual(attempted_payloads[first_cycle_calls], (source_png, "image/png"))
        self.assertEqual(variant, "original")
        self.assertEqual(file_bytes, source_png)
        self.assertEqual(mime_type, "image/png")
        self.assertEqual(pages[0]["text"], "# Merchant Receipt\n- Total: USD 12.40")

    def test_transcribe_pages_with_fallbacks_falls_back_to_markdown_transcription(self):
        source_png = self.make_png_bytes()
        markdown_seen = []

        def fake_generate(_, **kwargs):
            config = kwargs["config"]
            response_mime = config["response_mime_type"]
            if response_mime == "application/json":
                return type("Response", (), {"text": ""})()
            markdown_seen.append(kwargs["contents"][1])
            return type(
                "Response",
                (),
                {"text": "# Merchant Receipt\n- Total: USD 12.40"},
            )()

        pages, variant, file_bytes, mime_type = transcribe.transcribe_pages_with_fallbacks(
            object(),
            model="gemini-3-flash-preview",
            prompt=transcribe.build_prompt("receipt.png", "image/png", "primary"),
            markdown_fallback_prompt=transcribe.build_markdown_fallback_prompt(
                "receipt.png", "image/png", "primary"
            ),
            document_path=Path("receipt.png"),
            source_file_bytes=source_png,
            source_mime_type="image/png",
            primary_file_bytes=source_png,
            primary_mime_type="image/png",
            preprocess_variant="original",
            generate_fn=fake_generate,
            preprocess_fn=lambda *_args: (b"retry-bytes", "image/png"),
            part_factory=lambda data, detected_mime: (data, detected_mime),
        )

        self.assertGreaterEqual(len(markdown_seen), 1)
        self.assertEqual(variant, "original")
        self.assertEqual(file_bytes, source_png)
        self.assertEqual(mime_type, "image/png")
        self.assertEqual(pages[0]["text"], "# Merchant Receipt\n- Total: USD 12.40")

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

    def test_maybe_ground_key_receipt_fields_tolerates_dimension_probe_failure(self):
        pages = [{"page_number": 1, "text": "# Merchant Receipt", "dimensions": None, "regions": []}]

        def fake_generate(_, **_kwargs):
            return type(
                "Response",
                (),
                {
                    "text": json.dumps(
                        {
                            "regions": [
                                {
                                    "region_id": "merchant_name",
                                    "kind": "value_candidate",
                                    "text": "Corner Store",
                                    "box_2d": [50, 70, 110, 520],
                                }
                            ]
                        }
                    )
                },
            )()

        grounded_pages, geometry_source, geometry_available, grounding_variant = (
            transcribe.maybe_ground_key_receipt_fields(
                object(),
                model="gemini-3-flash-preview",
                filename="receipt.png",
                file_bytes=b"not-a-real-image",
                mime_type="image/png",
                normalized_pages=pages,
                document_path=Path("receipt.png"),
                source_file_bytes=b"not-a-real-image",
                source_mime_type="image/png",
                preprocess_variant="original",
                grounding_variants=["original"],
                generate_fn=fake_generate,
                preprocess_fn=lambda *_args: (b"retry-bytes", "image/png"),
                part_factory=lambda data, detected_mime: (data, detected_mime),
            )
        )

        self.assertEqual(geometry_source, "gemini")
        self.assertTrue(geometry_available)
        self.assertEqual(grounding_variant, "original")
        self.assertEqual(grounded_pages[0]["regions"][0]["region_id"], "merchant_name")

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

    def test_preprocess_document_bytes_accepts_heif_via_optional_opener(self):
        with mock.patch.object(
            transcribe,
            "register_optional_heif_support",
        ) as _register, mock.patch.object(
            transcribe,
            "open_image_for_ocr",
        ) as open_image:
            from PIL import Image

            open_image.return_value = Image.new("RGB", (12, 12), color=(200, 200, 200))
            processed_bytes, processed_mime = transcribe.preprocess_document_bytes(
                Path("receipt.png"),
                b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00payload",
                "image/heic",
                "original",
            )

        self.assertEqual(processed_mime, "image/png")
        self.assertGreater(len(processed_bytes), 0)

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
