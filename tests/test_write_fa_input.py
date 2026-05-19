"""Covers `write_fa_input` from scripts/local_app_simple.py.

This is the Python side of the FA-input wiring (Stage S.2): the helper
that takes the Flask form payload and writes `fa_input.json` in the
shape the Rust `FaInput` struct expects. The Rust side has its own
serde round-trip tests in src/fa_input.rs."""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from local_app_simple import write_fa_input  # noqa: E402


SCRATCH = REPO_ROOT / ".scratch" / "tests_write_fa_input"


class TestWriteFaInput(unittest.TestCase):
    def setUp(self) -> None:
        SCRATCH.mkdir(parents=True, exist_ok=True)
        self.out = SCRATCH / f"{self._testMethodName}.json"
        self.out.unlink(missing_ok=True)

    def tearDown(self) -> None:
        self.out.unlink(missing_ok=True)

    def test_happy_path_maps_form_keys_to_json_keys(self) -> None:
        form = {
            "fa_payee_name": "Jane Doe",
            "fa_payee_affiliation": "faculty",
            "fa_event_name": "ASPLOS 2026",
            "fa_bp_who": "Jane Doe + 2 collaborators",
            "fa_bp_what": "Presented research",
            "fa_bp_when_from": "2026-03-15",
            "fa_bp_when_to": "2026-03-19",
            "fa_bp_where": "San Diego, USA",
            "fa_bp_why": "Disseminating Stanford research",
            "fa_bp_key": "ASPLOS 2026 trip",
            "fa_authorized_by": "advisor@stanford.edu",
            "fa_rush_processing": "no",
            "fa_payment_method": "Personal",
            "fa_foreign_activity_type": "conference",
        }
        write_fa_input(form, self.out)
        data = json.loads(self.out.read_text())
        # JSON keys are the FaInput field names in src/fa_input.rs.
        self.assertEqual(data["payee_name"], "Jane Doe")
        self.assertEqual(data["payee_affiliation"], "faculty")
        self.assertEqual(data["event_name"], "ASPLOS 2026")
        self.assertEqual(data["business_purpose_who"], "Jane Doe + 2 collaborators")
        self.assertEqual(data["business_purpose_when"], "2026-03-15 to 2026-03-19")
        self.assertEqual(data["business_purpose_key_30char"], "ASPLOS 2026 trip")
        self.assertEqual(data["authorized_by"], "advisor@stanford.edu")
        self.assertEqual(data["rush_processing"], "no")
        self.assertEqual(data["payment_method"], "Personal")
        self.assertEqual(data["foreign_activity_type"], "conference")

    def test_single_day_when_collapses_to_one_date(self) -> None:
        # FA leaves "to" blank → just the start date, no " to " suffix.
        form = {"fa_payee_name": "Jane Doe", "fa_bp_when_from": "2026-03-15"}
        write_fa_input(form, self.out)
        data = json.loads(self.out.read_text())
        self.assertEqual(data["business_purpose_when"], "2026-03-15")

    def test_same_from_and_to_collapses_to_one_date(self) -> None:
        # FA picks the same date for both → render as one date, not
        # "2026-03-15 to 2026-03-15" which reads as a typo.
        form = {
            "fa_payee_name": "Jane Doe",
            "fa_bp_when_from": "2026-03-15",
            "fa_bp_when_to": "2026-03-15",
        }
        write_fa_input(form, self.out)
        data = json.loads(self.out.read_text())
        self.assertEqual(data["business_purpose_when"], "2026-03-15")

    def test_blank_string_values_are_dropped(self) -> None:
        # Optional fields can come through as empty strings when the FA
        # leaves a select on its placeholder. Drop them so the Rust side
        # sees Option::None, not Some("").
        form = {
            "fa_payee_name": "Jane Doe",
            "fa_payee_affiliation": "faculty",
            "fa_event_name": "",
            "fa_foreign_activity_type": "   ",
        }
        write_fa_input(form, self.out)
        data = json.loads(self.out.read_text())
        self.assertNotIn("event_name", data)
        self.assertNotIn("foreign_activity_type", data)
        self.assertEqual(data["payee_name"], "Jane Doe")

    def test_no_fa_keys_writes_nothing(self) -> None:
        # Programmatic POST that skips the fieldset entirely → no file,
        # reducer falls back to the no-FA-input flow.
        write_fa_input({"some_other_field": "x"}, self.out)
        self.assertFalse(self.out.exists())

    def test_unicode_and_quotes_round_trip(self) -> None:
        form = {
            "fa_payee_name": 'Jane "JD" Doe',
            "fa_bp_what": "Café visit — 你好",
        }
        write_fa_input(form, self.out)
        data = json.loads(self.out.read_text())
        self.assertEqual(data["payee_name"], 'Jane "JD" Doe')
        self.assertEqual(data["business_purpose_what"], "Café visit — 你好")


if __name__ == "__main__":
    unittest.main()
