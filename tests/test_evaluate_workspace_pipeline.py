import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


def load_module():
    script_path = (
        Path(__file__).resolve().parent.parent
        / "scripts"
        / "evaluate_workspace_pipeline.py"
    )
    spec = importlib.util.spec_from_file_location(
        "evaluate_workspace_pipeline", script_path
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


workspace_eval = load_module()


class EvaluateWorkspacePipelineTests(unittest.TestCase):
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

            comparison = workspace_eval.compare_markdown(source_path, transcribed_path)

            self.assertFalse(comparison["exact_match"])
            self.assertFalse(comparison["relaxed_match"])
            self.assertTrue(comparison["content_match"])

    def test_summarize_readiness_counts_issue_classes(self):
        summary = workspace_eval.summarize_readiness(
            {
                "issues": [
                    {"class": "automation_gap"},
                    {"class": "user_input_required"},
                    {"class": "manual_review"},
                    {"class": "manual_review"},
                ]
            }
        )

        self.assertEqual(summary["automation_gap_count"], 1)
        self.assertEqual(summary["user_input_gap_count"], 1)
        self.assertEqual(summary["manual_review_item_count"], 2)
        self.assertEqual(summary["other_warning_count"], 0)

    def test_render_report_markdown_lists_failures_and_packet_counts(self):
        markdown = workspace_eval.render_report_markdown(
            {
                "summary": {
                    "engine": "vertex-gemini-sdk",
                    "rendered_inputs": True,
                    "packet_count": 2,
                    "successful_packet_count": 1,
                    "failure_count": 1,
                    "document_count": 3,
                    "exact_match_count": 2,
                    "relaxed_match_count": 3,
                    "content_match_count": 3,
                    "filing_status_counts": {"user_input_required": 1},
                    "ledger_state_counts": {"user_input_required": 1},
                    "stage_counts": {"user_input_required": 1},
                },
                "failures": [
                    {
                        "packet_id": "synthetic_packet_0002",
                        "error": "command failed with exit code 1\n404 NOT_FOUND",
                    }
                ],
                "packets": [
                    {
                        "packet_id": "synthetic_packet_0001",
                        "current_stage": "user_input_required",
                        "filing_status": "user_input_required",
                        "ledger_state": "user_input_required",
                        "readiness": {
                            "automation_gap_count": 0,
                            "user_input_gap_count": 4,
                            "manual_review_item_count": 3,
                            "other_warning_count": 0,
                        },
                        "exact_match_count": 3,
                        "relaxed_match_count": 3,
                        "content_match_count": 3,
                    }
                ],
            }
        )

        self.assertIn("engine: vertex-gemini-sdk", markdown)
        self.assertIn("synthetic_packet_0002: 404 NOT_FOUND", markdown)
        self.assertIn("synthetic_packet_0001: stage=user_input_required", markdown)


if __name__ == "__main__":
    unittest.main()
