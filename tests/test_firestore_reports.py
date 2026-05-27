"""Real-Firestore tests for scripts/firestore_reports.py.

Mirror of test_firestore_jobs.py. Each test uses a unique
test_<uuid> id, then cleans up. Same skip conditions.
"""

from __future__ import annotations

import os
import sys
import unittest
import uuid
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

try:
    from google.cloud import firestore  # noqa: F401
    FIRESTORE_AVAILABLE = True
except ImportError:
    FIRESTORE_AVAILABLE = False


def _firestore_reachable() -> bool:
    if not FIRESTORE_AVAILABLE:
        return False
    if not os.environ.get("VERTEX_PROJECT_ID"):
        return False
    try:
        from firestore_reports import _get_client
        _get_client()
        return True
    except Exception:
        return False


@unittest.skipUnless(_firestore_reachable(),
                     "Firestore unreachable (SDK missing, "
                     "VERTEX_PROJECT_ID unset, or ADC not configured)")
class TestFirestoreReports(unittest.TestCase):

    def setUp(self):
        from firestore_reports import set_report, get_report, delete_report
        self.set_report = set_report
        self.get_report = get_report
        self.delete_report = delete_report
        self.upload_id = f"test_{uuid.uuid4().hex[:12]}"

    def tearDown(self):
        try:
            self.delete_report(self.upload_id)
        except Exception:
            pass

    def test_get_returns_none_for_unknown_id(self):
        self.assertIsNone(self.get_report(self.upload_id))

    def test_set_then_get_round_trip(self):
        report = {"expense_report": {"total_usd": {"value": 42.0}}}
        fa_input = {"fa_payee_sunet": "abc"}
        self.set_report(self.upload_id, report=report, fa_input=fa_input)
        got = self.get_report(self.upload_id)
        self.assertEqual(got["report"], report)
        self.assertEqual(got["fa_input"], fa_input)
        self.assertEqual(got["history"], [])
        self.assertNotIn("created_at", got)
        self.assertNotIn("updated_at", got)
        self.assertNotIn("ttl", got)

    def test_report_with_nested_arrays_round_trips(self):
        """Regression: Firestore rejects arrays-of-arrays as "invalid
        nested entity". Real reports carry bbox coordinate arrays
        from OCR-grounding (e.g. [[x1,y1],[x2,y2]]). The wire format
        JSON-encodes the report field so this round-trips cleanly."""
        report = {
            "expense_report": {
                "transaction_lines": [
                    {"bbox": [[100, 200], [150, 220]],
                     "token_ids": [[1, 2, 3], [4, 5]]},
                ],
            },
        }
        self.set_report(self.upload_id, report=report)
        got = self.get_report(self.upload_id)
        self.assertEqual(got["report"], report)

    def test_set_with_history(self):
        report = {"expense_report": {}}
        history = [
            {"path": "x.y", "old_value": "a", "new_value": "b",
             "ts": "2026-05-25T00:00:00Z"},
        ]
        self.set_report(self.upload_id, report=report, history=history)
        got = self.get_report(self.upload_id)
        self.assertEqual(got["history"], history)

    def test_history_capped_at_50(self):
        report = {"expense_report": {}}
        history = [{"path": f"p{i}", "old_value": "a", "new_value": "b"}
                   for i in range(75)]
        self.set_report(self.upload_id, report=report, history=history)
        got = self.get_report(self.upload_id)
        self.assertEqual(len(got["history"]), 50)
        self.assertEqual(got["history"][0]["path"], "p25",
                         "should keep the LAST 50 entries, not the first")
        self.assertEqual(got["history"][-1]["path"], "p74")

    def test_fa_input_optional(self):
        report = {"expense_report": {}}
        self.set_report(self.upload_id, report=report)
        got = self.get_report(self.upload_id)
        self.assertIsNone(got["fa_input"])

    def test_set_is_merge_overwrites_report(self):
        first = {"expense_report": {"total_usd": {"value": 10.0}}}
        second = {"expense_report": {"total_usd": {"value": 99.0}}}
        self.set_report(self.upload_id, report=first,
                        fa_input={"fa_payee_sunet": "abc"})
        self.set_report(self.upload_id, report=second)
        got = self.get_report(self.upload_id)
        self.assertEqual(got["report"], second,
                         "second set must overwrite report field")
        self.assertIsNone(got["fa_input"],
                          "second set's None fa_input must overwrite first's")

    def test_filed_by_sunet_round_trips_and_lists_scope(self):
        """filed_by_sunet stores on set_report + scopes list_reports."""
        from firestore_reports import list_reports
        scoped = f"scope_{uuid.uuid4().hex[:8]}"
        # Two docs with the scoping value, one without
        ids = [f"test_{uuid.uuid4().hex[:8]}" for _ in range(3)]
        try:
            self.set_report(ids[0], report={"expense_report": {}},
                            filed_by_sunet=scoped)
            self.set_report(ids[1], report={"expense_report": {}},
                            filed_by_sunet=scoped)
            self.set_report(ids[2], report={"expense_report": {}})
            scoped_listed = [r for r in list_reports(filed_by_sunet=scoped)
                             if r["upload_id"] in ids]
            self.assertEqual(len(scoped_listed), 2,
                             "list_reports should match exactly the scoped IDs")
            self.assertTrue(
                all(r["filed_by_sunet"] == scoped for r in scoped_listed))
        finally:
            for i in ids:
                try:
                    self.delete_report(i)
                except Exception:
                    pass

    def test_delete_removes_document(self):
        self.set_report(self.upload_id, report={"expense_report": {}})
        self.assertIsNotNone(self.get_report(self.upload_id))
        self.delete_report(self.upload_id)
        self.assertIsNone(self.get_report(self.upload_id))

    def test_delete_unknown_id_is_safe(self):
        self.delete_report(f"never-exists-{uuid.uuid4().hex[:8]}")


if __name__ == "__main__":
    unittest.main()
