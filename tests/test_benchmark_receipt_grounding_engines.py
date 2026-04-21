import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


def load_module():
    script_path = (
        Path(__file__).resolve().parent.parent
        / "scripts"
        / "benchmark_receipt_grounding_engines.py"
    )
    spec = importlib.util.spec_from_file_location(
        "benchmark_receipt_grounding_engines", script_path
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


benchmark = load_module()


class BenchmarkReceiptGroundingEnginesTests(unittest.TestCase):
    def test_compare_expected_text_fields_matches_receipt_values_in_joined_text(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            transcription_path = Path(temp_dir) / "receipt.json"
            transcription_path.write_text(
                json.dumps(
                    {
                        "pages": [
                            {
                                "page_number": 1,
                                "text": "BOOK TALK (TAMAN DAYA) SDN BHD\n25/12/2018\nTOTAL RM 80.90",
                            }
                        ]
                    }
                )
            )

            comparison = benchmark.compare_expected_text_fields(
                {
                    "merchant_name": "BOOK TALK (TAMAN DAYA) SDN BHD",
                    "transaction_date": "25/12/2018",
                    "total_paid": "80.90",
                    "total_paid_currency": "MYR",
                },
                transcription_path,
            )

            self.assertEqual(comparison["expected_field_count"], 4)
            self.assertEqual(comparison["matched_field_count"], 4)

    def test_merge_hybrid_transcription_uses_gemini_text_and_docai_regions(self):
        gemini = {
            "document_id": "receipt_a",
            "filename": "receipt.png",
            "metadata": {
                "producer": "google_genai_sdk",
                "geometry_source": "gemini",
                "geometry_available": True,
            },
            "pages": [
                {
                    "page_number": 1,
                    "text": "# Merchant Receipt\n- Merchant Name: BOOK TALK",
                    "dimensions": None,
                    "regions": [{"region_id": "merchant_name", "text": "BOOK TALK"}],
                }
            ],
        }
        docai = {
            "document_id": "receipt_a",
            "filename": "receipt.png",
            "metadata": {
                "producer": "document_ai_sdk",
                "geometry_source": "document_ai",
                "geometry_available": True,
            },
            "pages": [
                {
                    "page_number": 1,
                    "text": "BOOK TALK",
                    "dimensions": {"width": 1000, "height": 2000},
                    "regions": [{"region_id": "page_1_line_1", "text": "BOOK TALK"}],
                }
            ],
        }

        hybrid = benchmark.merge_hybrid_transcription(gemini, docai)

        self.assertEqual(hybrid["metadata"]["geometry_source"], "hybrid")
        self.assertEqual(hybrid["metadata"]["producer"], "hybrid_document_ai_geometry")
        self.assertEqual(hybrid["pages"][0]["text"], gemini["pages"][0]["text"])
        self.assertEqual(hybrid["pages"][0]["regions"], docai["pages"][0]["regions"])
        self.assertEqual(hybrid["pages"][0]["dimensions"], {"width": 1000, "height": 2000})

    def test_summarize_lane_results_counts_grounding_and_content(self):
        summary = benchmark.summarize_lane_results(
            "gemini",
            [
                {
                    "document_id": "a",
                    "markdown": {"content_match": True},
                    "text_fields": {"expected_field_count": 4, "matched_field_count": 4},
                    "grounding": {
                        "geometry_available": True,
                        "expected_field_count": 4,
                        "matched_field_count": 4,
                        "fields": [
                            {"field": "merchant_name", "matched": True},
                            {"field": "transaction_date", "matched": True},
                        ],
                    },
                    "error": None,
                },
                {
                    "document_id": "b",
                    "markdown": {"content_match": False},
                    "text_fields": {"expected_field_count": 4, "matched_field_count": 2},
                    "grounding": {
                        "geometry_available": False,
                        "expected_field_count": 4,
                        "matched_field_count": 1,
                        "fields": [{"field": "merchant_name", "matched": True}],
                    },
                    "error": None,
                },
                {"document_id": "c", "error": "failed"},
            ],
        )

        self.assertEqual(summary["available_document_count"], 2)
        self.assertEqual(summary["failed_document_count"], 1)
        self.assertEqual(summary["content_match_count"], 1)
        self.assertEqual(summary["text_matched_field_count"], 6)
        self.assertEqual(summary["grounding_matched_field_count"], 5)
        self.assertEqual(summary["grounding_available_document_count"], 1)

    def test_render_reports_include_lane_names(self):
        report = {
            "corpus_name": "receipt_seed",
            "document_count": 2,
            "lanes": [
                {
                    "lane": "gemini",
                    "summary": {
                        "lane": "gemini",
                        "document_count": 2,
                        "available_document_count": 2,
                        "content_match_count": 2,
                        "text_expected_field_count": 8,
                        "text_matched_field_count": 8,
                        "grounding_expected_field_count": 8,
                        "grounding_matched_field_count": 7,
                        "grounding_available_document_count": 2,
                        "grounding_fully_matched_document_count": 1,
                    },
                    "documents": [],
                }
            ],
        }

        markdown = benchmark.render_markdown_report(report)
        html = benchmark.render_html_report(report)

        self.assertIn("## gemini", markdown)
        self.assertIn("Receipt OCR Engine Benchmark", html)
        self.assertIn("gemini", html)

    def test_run_command_reports_timeout_cleanly(self):
        with mock.patch.object(
            benchmark.subprocess,
            "run",
            side_effect=benchmark.subprocess.TimeoutExpired(cmd=["cargo"], timeout=5),
        ):
            with self.assertRaises(benchmark.CommandError) as context:
                benchmark.run_command(["cargo", "run"], cwd=Path("."), timeout_seconds=5)

        self.assertIn("timed out after 5s", str(context.exception))

    def test_benchmark_lanes_respects_max_documents(self):
        args = type(
            "Args",
            (),
            {
                "lanes": ["gemini"],
                "max_documents": 1,
                "command_timeout_seconds": 5,
                "service_account_key": "key.json",
                "gemini_location": "global",
                "gemini_model": "gemini-3-flash-preview",
                "sdk_python": ".venv/bin/python",
                "project": None,
                "docai_location": "us",
                "docai_processor_id": None,
                "docai_processor_version": None,
                "output_dir": ".",
            },
        )()

        with mock.patch.object(
            benchmark,
            "run_lane_document",
            return_value={"document_id": "receipt_a", "error": None},
        ) as run_lane_document:
            report = benchmark.benchmark_lanes(
                {
                    "corpus_name": "receipt_seed",
                    "packets": [
                        {
                            "packet_id": "packet_a",
                            "documents": [
                                {"document_id": "receipt_a"},
                                {"document_id": "receipt_b"},
                            ],
                        }
                    ],
                },
                args,
            )

        self.assertEqual(report["document_count"], 1)
        self.assertEqual(run_lane_document.call_count, 1)


if __name__ == "__main__":
    unittest.main()
