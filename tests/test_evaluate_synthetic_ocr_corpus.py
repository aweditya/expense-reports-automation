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
    def test_compare_expected_fields_scores_receipt_facts(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            facts_path = temp_path / "receipt.facts.json"
            facts_path.write_text(
                json.dumps(
                    {
                        "classification": {"kind": "receipt"},
                        "facts": {
                            "receipt": {
                                "merchant_name": {"value": "EAST BAY BISTRO"},
                                "transaction_date": {"value": "2025-04-24"},
                                "total_paid": {
                                    "value": {"amount": "35.02", "currency": "SGD"}
                                },
                                "line_items": [{}, {}],
                            }
                        },
                    }
                )
            )

            comparison = ocr_eval.compare_expected_fields(
                {
                    "classification_kind": "receipt",
                    "merchant_name": "east bay bistro",
                    "transaction_date": "2025-04-24",
                    "total_paid": "35.02",
                    "total_paid_currency": "sgd",
                    "line_item_count": 2,
                },
                facts_path,
            )

            self.assertEqual(comparison["expected_field_count"], 6)
            self.assertEqual(comparison["matched_field_count"], 6)

    def test_compare_expected_fields_normalizes_merchant_name_and_date(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            facts_path = temp_path / "receipt.facts.json"
            facts_path.write_text(
                json.dumps(
                    {
                        "classification": {"kind": "receipt"},
                        "facts": {
                            "receipt": {
                                "merchant_name": {
                                    "value": "Gerbang Alaf Restaurants Sdn Bhd (65351-M)"
                                },
                                "transaction_date": {"value": "Friday, 29-12-2017"},
                            }
                        },
                    }
                )
            )

            comparison = ocr_eval.compare_expected_fields(
                {
                    "merchant_name": "GERBANG ALAF RESTAURANTS SDN BHD",
                    "transaction_date": "29-12-2017",
                },
                facts_path,
            )

            self.assertEqual(comparison["expected_field_count"], 2)
            self.assertEqual(comparison["matched_field_count"], 2)

    def test_compare_expected_grounding_scores_localized_receipt_regions(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            transcription_path = temp_path / "receipt.transcribed.json"
            transcription_path.write_text(
                json.dumps(
                    {
                        "metadata": {
                            "geometry_source": "gemini",
                            "geometry_available": True,
                        },
                        "pages": [
                            {
                                "page_number": 1,
                                "text": "# Receipt",
                                "regions": [
                                    {
                                        "region_id": "merchant_name",
                                        "text": "BOOK TALK (TAMAN DAYA) SDN BHD",
                                    },
                                    {
                                        "region_id": "transaction_date",
                                        "text": "25/12/2018",
                                    },
                                    {
                                        "region_id": "total_paid",
                                        "text": "RM 80.90",
                                    },
                                    {
                                        "region_id": "total_paid_currency",
                                        "text": "MYR",
                                    },
                                ],
                            }
                        ],
                    }
                )
            )

            comparison = ocr_eval.compare_expected_grounding(
                {
                    "merchant_name": "BOOK TALK (TAMAN DAYA) SDN BHD",
                    "transaction_date": "25/12/2018",
                    "total_paid": "80.90",
                    "total_paid_currency": "MYR",
                    "classification_kind": "receipt",
                },
                transcription_path,
            )

            self.assertIsNotNone(comparison)
            self.assertTrue(comparison["geometry_available"])
            self.assertEqual(comparison["expected_field_count"], 4)
            self.assertEqual(comparison["matched_field_count"], 4)

    def test_compare_expected_grounding_returns_none_when_no_groundable_fields_exist(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            transcription_path = temp_path / "receipt.transcribed.json"
            transcription_path.write_text(
                json.dumps(
                    {
                        "metadata": {
                            "geometry_source": "none",
                            "geometry_available": False,
                        },
                        "pages": [{"page_number": 1, "text": "# Receipt", "regions": []}],
                    }
                )
            )

            comparison = ocr_eval.compare_expected_grounding(
                {"classification_kind": "receipt"},
                transcription_path,
            )

            self.assertIsNone(comparison)

    def test_build_manifest_corpus_spec_from_flat_documents(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            corpus_root = temp_path / "receipt_corpus"
            manifest_dir = corpus_root / "manifests"
            manifest_dir.mkdir(parents=True)
            receipt_path = corpus_root / "receipt.pdf"
            markdown_path = corpus_root / "receipt.md"
            manifest_path = manifest_dir / "manifest.json"

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

    def test_summarize_pass_comparison_marks_divergence_and_confidence(self):
        summary = ocr_eval.summarize_pass_comparison(
            {
                "overall_confidence": "low",
                "disagreement_count": 2,
                "fields": [
                    {"field": "merchant_name", "confidence": "high", "status": "consensus"},
                    {"field": "total_paid", "confidence": "low", "status": "divergent"},
                ],
            }
        )

        self.assertEqual(summary["overall_confidence"], "low")
        self.assertEqual(summary["disagreement_count"], 2)
        self.assertTrue(summary["has_divergence"])
        self.assertEqual(summary["field_confidence_counts"]["high"], 1)
        self.assertEqual(summary["field_confidence_counts"]["low"], 1)
        self.assertEqual(summary["field_status_counts"]["consensus"], 1)
        self.assertEqual(summary["field_status_counts"]["divergent"], 1)
        self.assertEqual(summary["divergent_fields"], ["total_paid"])

    def test_resolve_compare_profiles_uses_defaults_and_validates_custom_profiles(self):
        defaults = ocr_eval.resolve_compare_profiles(
            type("Args", (), {"compare_profiles": None})()
        )

        self.assertEqual(
            [profile["name"] for profile in defaults],
            ["table_focused_binarized", "verification_contrast_boosted"],
        )

        custom = ocr_eval.resolve_compare_profiles(
            type(
                "Args",
                (),
                {"compare_profiles": ["deskew_check:verification:deskewed"]},
            )()
        )

        self.assertEqual(custom[0]["pass_kind"], "verification")
        self.assertEqual(custom[0]["preprocess_variant"], "deskewed")

        with self.assertRaises(ValueError):
            ocr_eval.resolve_compare_profiles(
                type("Args", (), {"compare_profiles": ["badprofile"]})()
            )

    def test_summarize_comparison_preserves_per_model_totals(self):
        comparison = ocr_eval.summarize_comparison(
            [
                {
                    "summary": {
                        "model": "gemini-3-flash-preview",
                        "model_key": "gemini_3_flash_preview",
                        "packet_count": 4,
                        "document_count": 12,
                        "grounding_document_count": 3,
                        "grounding_available_document_count": 2,
                        "grounding_fully_matched_document_count": 2,
                        "grounding_expected_field_count": 12,
                        "grounding_matched_field_count": 9,
                        "grounding_field_match_counts": {
                            "merchant_name": 3,
                            "transaction_date": 2,
                        },
                        "pass_comparison_document_count": 3,
                        "pass_comparison_divergent_document_count": 1,
                        "pass_comparison_disagreement_count": 2,
                        "pass_comparison_confidence_counts": {"high": 2, "low": 1},
                        "pass_comparison_field_confidence_counts": {"high": 4, "low": 1},
                        "pass_comparison_field_status_counts": {
                            "consensus": 4,
                            "divergent": 1,
                        },
                        "exact_match_count": 10,
                        "relaxed_match_count": 11,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                        "inspection_document_count": 12,
                    }
                },
                {
                    "summary": {
                        "model": "gemini-3-pro-preview",
                        "model_key": "gemini_3_pro_preview",
                        "packet_count": 4,
                        "document_count": 12,
                        "grounding_document_count": 3,
                        "grounding_available_document_count": 3,
                        "grounding_fully_matched_document_count": 3,
                        "grounding_expected_field_count": 12,
                        "grounding_matched_field_count": 12,
                        "grounding_field_match_counts": {
                            "merchant_name": 3,
                            "transaction_date": 3,
                        },
                        "pass_comparison_document_count": 3,
                        "pass_comparison_divergent_document_count": 0,
                        "pass_comparison_disagreement_count": 0,
                        "pass_comparison_confidence_counts": {"high": 3},
                        "pass_comparison_field_confidence_counts": {"high": 5},
                        "pass_comparison_field_status_counts": {"consensus": 5},
                        "exact_match_count": 11,
                        "relaxed_match_count": 12,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                        "inspection_document_count": 12,
                    }
                },
            ]
        )

        self.assertEqual(len(comparison["models"]), 2)
        self.assertEqual(comparison["models"][0]["exact_match_count"], 10)
        self.assertEqual(comparison["models"][1]["exact_match_count"], 11)
        self.assertEqual(comparison["models"][0]["grounding_matched_field_count"], 9)
        self.assertEqual(comparison["models"][1]["grounding_available_document_count"], 3)
        self.assertEqual(comparison["models"][0]["pass_comparison_disagreement_count"], 2)
        self.assertEqual(comparison["models"][1]["pass_comparison_confidence_counts"]["high"], 3)
        self.assertEqual(
            comparison["models"][0]["pass_comparison_field_status_counts"]["divergent"], 1
        )
        self.assertEqual(comparison["models"][0]["inspection_document_count"], 12)

    def test_render_comparison_markdown_lists_models(self):
        markdown = ocr_eval.render_comparison_markdown(
            {
                "models": [
                    {
                        "model": "gemini-3-flash-preview",
                        "document_count": 12,
                        "grounding_document_count": 3,
                        "grounding_available_document_count": 2,
                        "grounding_fully_matched_document_count": 2,
                        "grounding_expected_field_count": 12,
                        "grounding_matched_field_count": 9,
                        "grounding_field_match_counts": {"merchant_name": 3},
                        "pass_comparison_document_count": 3,
                        "pass_comparison_divergent_document_count": 1,
                        "pass_comparison_disagreement_count": 2,
                        "pass_comparison_confidence_counts": {"high": 2, "low": 1},
                        "pass_comparison_field_confidence_counts": {"high": 4, "low": 1},
                        "pass_comparison_field_status_counts": {
                            "consensus": 4,
                            "divergent": 1,
                        },
                        "exact_match_count": 10,
                        "relaxed_match_count": 11,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                        "inspection_document_count": 12,
                    },
                    {
                        "model": "gemini-3-pro-preview",
                        "document_count": 12,
                        "pass_comparison_document_count": 3,
                        "pass_comparison_divergent_document_count": 0,
                        "pass_comparison_disagreement_count": 0,
                        "pass_comparison_confidence_counts": {"high": 3},
                        "pass_comparison_field_confidence_counts": {"high": 5},
                        "pass_comparison_field_status_counts": {"consensus": 5},
                        "exact_match_count": 11,
                        "relaxed_match_count": 12,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                        "inspection_document_count": 12,
                    },
                ]
            }
        )

        self.assertIn("gemini-3-flash-preview", markdown)
        self.assertIn("gemini-3-pro-preview", markdown)
        self.assertIn("pass_disagreements=2", markdown)
        self.assertIn("confidence=high=2, low=1", markdown)
        self.assertIn("field status counts", ocr_eval.render_model_report_markdown(
            {
                "summary": {
                    "status": "ok",
                    "corpus_name": "receipt_seed",
                    "packet_count": 0,
                    "document_count": 0,
                    "comparable_document_count": 0,
                    "expected_field_count": 0,
                    "matched_field_count": 0,
                    "grounding_document_count": 0,
                    "grounding_available_document_count": 0,
                    "grounding_fully_matched_document_count": 0,
                    "grounding_expected_field_count": 0,
                    "grounding_matched_field_count": 0,
                    "grounding_field_match_counts": {},
                    "pass_comparison_document_count": 1,
                    "pass_comparison_divergent_document_count": 1,
                    "pass_comparison_disagreement_count": 1,
                    "pass_comparison_confidence_counts": {"low": 1},
                    "pass_comparison_field_confidence_counts": {"high": 2, "low": 1},
                    "pass_comparison_field_status_counts": {"consensus": 2, "divergent": 1},
                    "inspection_document_count": 1,
                    "model": "gemini-3-flash-preview",
                    "exact_match_count": 0,
                    "relaxed_match_count": 0,
                    "content_match_count": 0,
                    "filing_status_counts": {},
                    "ledger_state_counts": {},
                },
                "packets": [],
            }
        ))

    def test_render_comparison_markdown_surfaces_model_errors(self):
        markdown = ocr_eval.render_comparison_markdown(
            {
                "models": [
                    {
                        "model": "gemini-3-flash-preview",
                        "status": "ok",
                        "document_count": 12,
                        "grounding_document_count": 3,
                        "grounding_available_document_count": 2,
                        "grounding_fully_matched_document_count": 2,
                        "grounding_expected_field_count": 12,
                        "grounding_matched_field_count": 9,
                        "grounding_field_match_counts": {"merchant_name": 3},
                        "pass_comparison_document_count": 0,
                        "pass_comparison_divergent_document_count": 0,
                        "pass_comparison_disagreement_count": 0,
                        "pass_comparison_confidence_counts": {},
                        "pass_comparison_field_confidence_counts": {},
                        "pass_comparison_field_status_counts": {},
                        "exact_match_count": 10,
                        "relaxed_match_count": 11,
                        "content_match_count": 12,
                        "filing_status_counts": {"userinputrequired": 4},
                        "ledger_state_counts": {"userinputrequired": 4},
                        "inspection_document_count": 12,
                    },
                    {
                        "model": "gemini-3-pro-preview",
                        "status": "error",
                        "error": "command failed with exit code 1\n404 NOT_FOUND",
                        "error_summary": "404 NOT_FOUND",
                        "document_count": 0,
                        "grounding_document_count": 0,
                        "grounding_available_document_count": 0,
                        "grounding_fully_matched_document_count": 0,
                        "grounding_expected_field_count": 0,
                        "grounding_matched_field_count": 0,
                        "grounding_field_match_counts": {},
                        "pass_comparison_document_count": 0,
                        "pass_comparison_divergent_document_count": 0,
                        "pass_comparison_disagreement_count": 0,
                        "pass_comparison_confidence_counts": {},
                        "pass_comparison_field_confidence_counts": {},
                        "pass_comparison_field_status_counts": {},
                        "exact_match_count": 0,
                        "relaxed_match_count": 0,
                        "content_match_count": 0,
                        "filing_status_counts": {},
                        "ledger_state_counts": {},
                        "inspection_document_count": 0,
                    },
                ]
            }
        )

        self.assertIn("gemini-3-pro-preview: error=404 NOT_FOUND", markdown)
        self.assertIn("gemini-3-pro-preview: unavailable", markdown)
        self.assertIn("grounded=9/12", markdown)

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
        self.assertEqual(report["summary"]["grounding_document_count"], 0)
        self.assertEqual(report["summary"]["pass_comparison_document_count"], 0)
        self.assertEqual(report["summary"]["inspection_document_count"], 0)
        self.assertEqual(report["summary"]["exact_match_count"], 0)

    def test_render_model_report_markdown_includes_pass_comparison_summary(self):
        markdown = ocr_eval.render_model_report_markdown(
            {
                "summary": {
                    "status": "ok",
                    "corpus_name": "receipt_seed",
                    "packet_count": 2,
                    "document_count": 4,
                    "comparable_document_count": 4,
                    "expected_field_count": 8,
                    "matched_field_count": 8,
                    "grounding_document_count": 2,
                    "grounding_available_document_count": 2,
                    "grounding_fully_matched_document_count": 1,
                    "grounding_expected_field_count": 8,
                    "grounding_matched_field_count": 6,
                    "grounding_field_match_counts": {
                        "merchant_name": 2,
                        "transaction_date": 1,
                    },
                    "pass_comparison_document_count": 2,
                    "pass_comparison_divergent_document_count": 1,
                    "pass_comparison_disagreement_count": 3,
                    "pass_comparison_confidence_counts": {"high": 1, "low": 1},
                    "pass_comparison_field_confidence_counts": {"high": 2, "low": 1},
                    "pass_comparison_field_status_counts": {
                        "consensus": 2,
                        "divergent": 1,
                    },
                    "pass_comparison_profile_run_count": 4,
                    "pass_comparison_profile_summaries": {
                        "table_focused_binarized": {
                            "run_count": 2,
                            "divergent_run_count": 1,
                            "disagreement_count": 3,
                            "confidence_counts": {"high": 1, "low": 1},
                            "field_confidence_counts": {"high": 2, "low": 1},
                            "field_status_counts": {"consensus": 2, "divergent": 1},
                        }
                    },
                    "inspection_document_count": 2,
                    "model": "gemini-3-flash-preview",
                    "exact_match_count": 4,
                    "relaxed_match_count": 4,
                    "content_match_count": 4,
                    "filing_status_counts": {"userinputrequired": 2},
                    "ledger_state_counts": {"userinputrequired": 2},
                },
                "packets": [],
            }
        )

        self.assertIn("- grounded receipt docs: 2", markdown)
        self.assertIn("- docs with OCR geometry: 2", markdown)
        self.assertIn("- fully grounded docs: 1", markdown)
        self.assertIn("- matched grounded fields: 6/8", markdown)
        self.assertIn("- merchant_name: 2", markdown)
        self.assertIn("- pass comparisons: 2", markdown)
        self.assertIn("- pass profile runs: 4", markdown)
        self.assertIn("- OCR inspections: 2", markdown)
        self.assertIn("- pass-comparison divergences: 1", markdown)
        self.assertIn("- total field disagreements: 3", markdown)
        self.assertIn("- high: 1", markdown)
        self.assertIn("- low: 1", markdown)
        self.assertIn("- field confidence counts:", markdown)
        self.assertIn("- field status counts:", markdown)
        self.assertIn("table_focused_binarized: runs=2", markdown)

    def test_render_model_report_html_includes_artifact_links(self):
        html = ocr_eval.render_model_report_html(
            {
                "summary": {
                    "status": "ok",
                    "corpus_name": "receipt_seed",
                    "packet_count": 1,
                    "document_count": 1,
                    "comparable_document_count": 1,
                    "expected_field_count": 4,
                    "matched_field_count": 4,
                    "grounding_document_count": 1,
                    "grounding_available_document_count": 1,
                    "grounding_fully_matched_document_count": 1,
                    "grounding_expected_field_count": 4,
                    "grounding_matched_field_count": 4,
                    "grounding_field_match_counts": {"merchant_name": 1},
                    "pass_comparison_document_count": 1,
                    "pass_comparison_profile_run_count": 2,
                    "pass_comparison_divergent_document_count": 0,
                    "pass_comparison_disagreement_count": 0,
                    "pass_comparison_confidence_counts": {"high": 1},
                    "pass_comparison_field_confidence_counts": {"high": 5},
                    "pass_comparison_field_status_counts": {"consensus": 5},
                    "pass_comparison_profile_summaries": {
                        "table_focused_binarized": {
                            "run_count": 1,
                            "divergent_run_count": 0,
                            "disagreement_count": 0,
                            "confidence_counts": {"high": 1},
                            "field_confidence_counts": {"high": 5},
                            "field_status_counts": {"consensus": 5},
                        }
                    },
                    "inspection_document_count": 1,
                    "model": "gemini-3-flash-preview",
                    "exact_match_count": 1,
                    "relaxed_match_count": 1,
                    "content_match_count": 1,
                    "filing_status_counts": {"userinputrequired": 1},
                    "ledger_state_counts": {"userinputrequired": 1},
                },
                "packets": [
                    {
                        "packet_id": "packet_001",
                        "filing_status": "userinputrequired",
                        "ledger_state": "userinputrequired",
                        "readiness": {
                            "automation_gap_count": 0,
                            "user_input_gap_count": 1,
                            "manual_review_item_count": 0,
                            "other_warning_count": 0,
                        },
                        "documents": [
                            {
                                "document_id": "receipt_001",
                                "kind": "receipt",
                                "input_document": "receipt.png",
                                "transcription_json": "/repo/out/transcription.json",
                                "facts_json": "/repo/out/facts.json",
                                "ocr_inspection_html": "/repo/out/inspection.html",
                                "ocr_grounding_html": "/repo/out/grounding.html",
                                "ocr_pass_comparison_html": "/repo/out/comparison.html",
                                "ocr_pass_comparison_profiles": [
                                    {
                                        "name": "table_focused_binarized",
                                        "html_path": "/repo/out/comparison_table.html",
                                        "summary": {
                                            "overall_confidence": "high",
                                            "disagreement_count": 0,
                                        },
                                    },
                                    {
                                        "name": "verification_contrast_boosted",
                                        "html_path": "/repo/out/comparison_verify.html",
                                        "summary": {
                                            "overall_confidence": "medium",
                                            "disagreement_count": 1,
                                        },
                                    },
                                ],
                                "exact_match": True,
                                "relaxed_match": True,
                                "content_match": True,
                                "expected_fields": {
                                    "matched_field_count": 4,
                                    "expected_field_count": 4,
                                },
                                "expected_grounding": {
                                    "matched_field_count": 4,
                                    "expected_field_count": 4,
                                },
                                "ocr_pass_comparison": {
                                    "comparison": {
                                        "overall_confidence": "high",
                                        "disagreement_count": 0,
                                    }
                                },
                            }
                        ],
                    }
                ],
            }
        )

        self.assertIn("OCR Evaluation", html)
        self.assertIn("OCR Pass Profile Breakdown", html)
        self.assertIn("inspection", html)
        self.assertIn("pass diff (table_focused_binarized)", html)
        self.assertIn("pass diff (verification_contrast_boosted)", html)
        self.assertIn("table_focused_binarized=high/0", html)
        self.assertIn("table_focused_binarized", html)
        self.assertIn("file:///repo/out/inspection.html", html)


if __name__ == "__main__":
    unittest.main()
