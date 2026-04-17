import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


def load_module():
    script_path = (
        Path(__file__).resolve().parent.parent
        / "scripts"
        / "evaluate_synthetic_ocr_corpus.py"
    )
    spec = importlib.util.spec_from_file_location(
        "evaluate_synthetic_ocr_corpus", script_path
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


ocr_eval = load_module()


class EvaluateSyntheticOcrCorpusTests(unittest.TestCase):
    def test_build_manifest_corpus_spec_from_flat_documents(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            receipt_path = temp_path / "receipt.pdf"
            markdown_path = temp_path / "receipt.md"
            manifest_path = temp_path / "manifest.json"

            receipt_path.write_bytes(b"%PDF-1.4")
            markdown_path.write_text("# Merchant Receipt\n- Total: USD 12.40\n")
            manifest_path.write_text(
                json.dumps(
                    {
                        "corpus_name": "real_receipt_seed",
                        "documents": [
                            {
                                "document_id": "receipt_a",
                                "kind": "receipt",
                                "input_path": "receipt.pdf",
                                "ground_truth_markdown_path": "receipt.md",
                            }
                        ],
                    }
                )
            )

            corpus_spec = ocr_eval.build_manifest_corpus_spec(manifest_path)

            self.assertEqual(corpus_spec["corpus_name"], "real_receipt_seed")
            self.assertEqual(len(corpus_spec["packets"]), 1)
            packet = corpus_spec["packets"][0]
            self.assertEqual(packet["packet_id"], "receipt_a")
            self.assertEqual(packet["documents"][0]["kind"], "receipt")
            self.assertEqual(packet["documents"][0]["input_path"], str(receipt_path.resolve()))
            self.assertEqual(
                packet["documents"][0]["ground_truth_markdown_path"],
                str(markdown_path.resolve()),
            )
            self.assertEqual(packet["documents"][0]["transcription_stem"], "receipt")

    def test_compare_markdown_distinguishes_exact_relaxed_and_content(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            source_path = temp_path / "source.md"
            transcribed_path = temp_path / "transcribed.json"
            source_path.write_text("# Hotel Folio\n\n## Stay Summary\n- Guest Name: Olivia Park\n")
            transcribed_path.write_text(
                json.dumps(
                    {
                        "pages": [
                            {
                                "page_number": 1,
                                "text": "# Hotel Folio\n\n## Stay Summary\n\n- Guest Name: Olivia Park",
                            }
                        ]
                    }
                )
            )

            comparison = ocr_eval.compare_markdown(source_path, transcribed_path)

            self.assertFalse(comparison["exact_match"])
            self.assertFalse(comparison["relaxed_match"])
            self.assertTrue(comparison["content_match"])

    def test_compare_markdown_relaxed_match_tolerates_repeated_blank_lines(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            source_path = temp_path / "source.md"
            transcribed_path = temp_path / "transcribed.json"
            source_path.write_text("# Receipt\n\n- Total: USD 12.40\n")
            transcribed_path.write_text(
                json.dumps(
                    {
                        "pages": [
                            {
                                "page_number": 1,
                                "text": "# Receipt\n\n\n- Total: USD 12.40",
                            }
                        ]
                    }
                )
            )

            comparison = ocr_eval.compare_markdown(source_path, transcribed_path)

            self.assertFalse(comparison["exact_match"])
            self.assertTrue(comparison["relaxed_match"])
            self.assertTrue(comparison["content_match"])

    def test_summarize_readiness_counts_issue_classes(self):
        summary = ocr_eval.summarize_readiness(
            {
                "issues": [
                    {"class": "automation_gap"},
                    {"class": "user_input_required"},
                    {"class": "user_input_required"},
                    {"class": "manual_review"},
                ]
            }
        )

        self.assertEqual(summary["automation_gap_count"], 1)
        self.assertEqual(summary["user_input_gap_count"], 2)
        self.assertEqual(summary["manual_review_item_count"], 1)
        self.assertEqual(summary["other_warning_count"], 0)

    def test_summarize_comparison_preserves_per_model_totals(self):
        comparison = ocr_eval.summarize_comparison(
            [
                {
                    "summary": {
                        "model": "gemini-3-flash-preview",
                        "model_key": "gemini_3_flash_preview",
                        "packet_count": 4,
                        "document_count": 12,
                        "exact_match_count": 10,
                        "relaxed_match_count": 11,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                    }
                },
                {
                    "summary": {
                        "model": "gemini-3-pro-preview",
                        "model_key": "gemini_3_pro_preview",
                        "packet_count": 4,
                        "document_count": 12,
                        "exact_match_count": 11,
                        "relaxed_match_count": 12,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                    }
                },
            ]
        )

        self.assertEqual(len(comparison["models"]), 2)
        self.assertEqual(comparison["models"][0]["exact_match_count"], 10)
        self.assertEqual(comparison["models"][1]["exact_match_count"], 11)

    def test_render_comparison_markdown_lists_models(self):
        markdown = ocr_eval.render_comparison_markdown(
            {
                "models": [
                    {
                        "model": "gemini-3-flash-preview",
                        "document_count": 12,
                        "exact_match_count": 10,
                        "relaxed_match_count": 11,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                    },
                    {
                        "model": "gemini-3-pro-preview",
                        "document_count": 12,
                        "exact_match_count": 11,
                        "relaxed_match_count": 12,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                    },
                ]
            }
        )

        self.assertIn("gemini-3-flash-preview", markdown)
        self.assertIn("gemini-3-pro-preview", markdown)

    def test_render_comparison_markdown_surfaces_model_errors(self):
        markdown = ocr_eval.render_comparison_markdown(
            {
                "models": [
                    {
                        "model": "gemini-3-flash-preview",
                        "status": "ok",
                        "document_count": 12,
                        "exact_match_count": 10,
                        "relaxed_match_count": 11,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                    },
                    {
                        "model": "gemini-3-pro-preview",
                        "status": "error",
                        "error": "command failed with exit code 1\n404 NOT_FOUND",
                        "error_summary": "404 NOT_FOUND",
                        "document_count": 0,
                        "exact_match_count": 0,
                        "relaxed_match_count": 0,
                        "content_match_count": 0,
                        "filing_status_counts": {},
                        "ledger_state_counts": {},
                    },
                ]
            }
        )

        self.assertIn("gemini-3-pro-preview: error=404 NOT_FOUND", markdown)
        self.assertIn("gemini-3-pro-preview: unavailable", markdown)

    def test_condense_error_message_prefers_404_marker(self):
        error = "command failed with exit code 1\nupstream detail\n404 NOT_FOUND\nextra context"

        condensed = ocr_eval.condense_error_message(error)

        self.assertEqual(condensed, "404 NOT_FOUND")

    def test_failed_model_report_has_zeroed_summary_counts(self):
        report = ocr_eval.failed_model_report(
            "gemini-3-pro-preview",
            "global",
            "command failed with exit code 1\n404 NOT_FOUND",
        )

        self.assertEqual(report["summary"]["status"], "error")
        self.assertEqual(report["summary"]["error_summary"], "404 NOT_FOUND")
        self.assertEqual(report["summary"]["packet_count"], 0)
        self.assertEqual(report["summary"]["document_count"], 0)
        self.assertEqual(report["summary"]["exact_match_count"], 0)
        self.assertEqual(report["summary"]["relaxed_match_count"], 0)
        self.assertEqual(report["summary"]["content_match_count"], 0)
        self.assertEqual(report["packets"], [])


if __name__ == "__main__":
    unittest.main()
