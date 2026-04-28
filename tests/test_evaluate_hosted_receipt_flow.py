import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import scripts.evaluate_hosted_receipt_flow as hosted_eval


class HostedReceiptFlowEvalTests(unittest.TestCase):
    def test_resolve_manifest_document_paths_handles_relative_assets(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            manifest_dir = root / "manifests"
            assets_dir = root / "assets" / "demo"
            manifest_dir.mkdir(parents=True)
            assets_dir.mkdir(parents=True)
            receipt_path = assets_dir / "receipt.png"
            receipt_path.write_bytes(b"fake")
            manifest_path = manifest_dir / "demo.json"
            manifest_path.write_text(
                json.dumps(
                    {
                        "corpus_name": "demo",
                        "documents": [
                            {
                                "document_id": "receipt_1",
                                "input_path": "../assets/demo/receipt.png",
                                "expected_fields": {"classification_kind": "receipt"},
                            }
                        ],
                    }
                )
            )

            documents = hosted_eval.resolve_manifest_document_paths(manifest_path)

            self.assertEqual(len(documents), 1)
            self.assertEqual(documents[0]["document_id"], "receipt_1")
            self.assertEqual(documents[0]["input_path"], receipt_path.resolve())

    def test_resolve_manifest_document_paths_falls_back_to_corpus_root(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            manifest_dir = root / "manifests"
            assets_dir = root / "assets" / "demo"
            manifest_dir.mkdir(parents=True)
            assets_dir.mkdir(parents=True)
            receipt_path = assets_dir / "receipt.png"
            receipt_path.write_bytes(b"fake")
            manifest_path = manifest_dir / "demo.json"
            manifest_path.write_text(
                json.dumps(
                    {
                        "corpus_name": "demo",
                        "documents": [
                            {
                                "document_id": "receipt_2",
                                "input_path": "assets/demo/receipt.png",
                            }
                        ],
                    }
                )
            )

            documents = hosted_eval.resolve_manifest_document_paths(manifest_path)

            self.assertEqual(documents[0]["input_path"], receipt_path.resolve())

    def test_draft_line_summaries_extracts_projected_fields(self):
        draft_yaml = """
expense_report:
  transaction_lines:
    - common:
        expense_type:
          value: business_meal
        date:
          value: 2026-05-03
        line_amount_usd:
          value: '329.12'
        original_amount:
          value: '329.12'
        original_currency:
          value: USD
        remarks:
          value: Meal at Tamarine
"""
        lines = hosted_eval.draft_line_summaries(draft_yaml)
        self.assertEqual(
            lines,
            [
                {
                    "expense_type": "business_meal",
                    "date": "2026-05-03",
                    "line_amount_usd": "329.12",
                    "original_amount": "329.12",
                    "original_currency": "USD",
                    "remarks": "Meal at Tamarine",
                }
            ],
        )

    def test_classify_hosted_result_marks_user_viable_projection(self):
        result = hosted_eval.classify_hosted_result(
            filing_status="user_input_required",
            readiness={
                "automation_gap_count": 0,
                "user_input_gap_count": 5,
                "manual_review_count": 1,
            },
            draft_lines=[
                {
                    "expense_type": "business_meal",
                    "date": "2026-05-03",
                    "line_amount_usd": "329.12",
                    "original_amount": "329.12",
                    "original_currency": "USD",
                    "remarks": "Meal at Tamarine",
                }
            ],
        )
        self.assertTrue(result["hosted_ocr_ok"])
        self.assertTrue(result["schema_projected"])
        self.assertTrue(result["projected_without_automation_gaps"])
        self.assertTrue(result["ready_for_fa_completion"])

    def test_classify_hosted_result_flags_unprojected_automation_block(self):
        result = hosted_eval.classify_hosted_result(
            filing_status="automation_blocked",
            readiness={
                "automation_gap_count": 2,
                "user_input_gap_count": 0,
                "manual_review_count": 0,
            },
            draft_lines=[],
        )
        self.assertFalse(result["hosted_ocr_ok"])
        self.assertFalse(result["schema_projected"])
        self.assertFalse(result["projected_without_automation_gaps"])
        self.assertFalse(result["ready_for_fa_completion"])

    def test_evaluate_document_records_upload_failure_instead_of_aborting(self):
        document = {
            "document_id": "receipt_3",
            "input_path": Path("/fake/receipt.png"),
            "expected_fields": {},
        }

        with mock.patch.object(
            hosted_eval.e2e_test,
            "upload_documents",
            side_effect=SystemExit("upload failed"),
        ):
            result = hosted_eval.evaluate_document(
                base_url="https://example.com",
                headers={},
                document=document,
                bundle_prefix="hosted-test",
            )

        self.assertEqual(result["bundle_id"], "hosted-test_receipt_3")
        self.assertEqual(result["classification"]["automation_gap_count"], 1)
        self.assertFalse(result["classification"]["hosted_ocr_ok"])
        self.assertEqual(result["error"], "upload failed")

    def test_main_refreshes_cloud_run_auth_headers_for_each_document(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            manifest_path = root / "corpus.json"
            output_dir = root / "out"
            receipt_a = root / "a.png"
            receipt_b = root / "b.png"
            receipt_a.write_bytes(b"a")
            receipt_b.write_bytes(b"b")
            manifest_path.write_text(
                json.dumps(
                    {
                        "corpus_name": "demo",
                        "documents": [
                            {"document_id": "a", "input_path": str(receipt_a)},
                            {"document_id": "b", "input_path": str(receipt_b)},
                        ],
                    }
                )
            )

            seen_headers = []

            def fake_evaluate_document(*, base_url, headers, document, bundle_prefix):
                seen_headers.append(headers)
                return {
                    "document_id": document["document_id"],
                    "input_path": str(document["input_path"]),
                    "source_name": None,
                    "source_url": None,
                    "expected_fields": {},
                    "bundle_id": f"{bundle_prefix}_{document['document_id']}",
                    "upload_seconds": 1.0,
                    "filing_status": "user_input_required",
                    "readiness": {
                        "automation_gap_count": 0,
                        "user_input_gap_count": 1,
                        "manual_review_count": 0,
                    },
                    "manifest_document_count": 1,
                    "manifest_run_count": 1,
                    "draft_lines": [
                        {
                            "expense_type": "other_business_expense",
                            "date": "2026-04-27",
                            "line_amount_usd": "10.00",
                            "original_amount": "10.00",
                            "original_currency": "USD",
                            "remarks": "demo",
                        }
                    ],
                    "classification": {
                        "transaction_line_count": 1,
                        "expense_types": ["other_business_expense"],
                        "projected_core_fields": {
                            "date": True,
                            "line_amount_usd": True,
                            "original_amount": True,
                            "remarks": True,
                        },
                        "automation_gap_count": 0,
                        "user_input_gap_count": 1,
                        "manual_review_count": 0,
                        "hosted_ocr_ok": True,
                        "schema_projected": True,
                        "projected_without_automation_gaps": True,
                        "ready_for_fa_completion": True,
                    },
                    "error": None,
                }

            with mock.patch("sys.argv", [
                "evaluate_hosted_receipt_flow.py",
                "--cloud-run-url", "https://example.run.app",
                "--sa-key", "fake-key.json",
                "--corpus-manifest", str(manifest_path),
                "--output-dir", str(output_dir),
                "--bundle-prefix", "hosted-test",
            ]), mock.patch.object(
                hosted_eval,
                "build_auth_headers",
                side_effect=[
                    {"Authorization": "Bearer token-a"},
                    {"Authorization": "Bearer token-b"},
                ],
            ) as build_headers, mock.patch.object(
                hosted_eval,
                "evaluate_document",
                side_effect=fake_evaluate_document,
            ):
                exit_code = hosted_eval.main()

            self.assertEqual(exit_code, 0)
            self.assertEqual(
                seen_headers,
                [
                    {"Authorization": "Bearer token-a"},
                    {"Authorization": "Bearer token-b"},
                ],
            )
            self.assertEqual(build_headers.call_count, 2)


if __name__ == "__main__":
    unittest.main()
