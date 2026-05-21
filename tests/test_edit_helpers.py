"""Robustness tests for the Stage 7 edit-in-place helpers.

These cover the logic the FA's click-to-edit relies on:
  - _walk_to_leaf: path → (existing, setter) for Wrapped/bare/missing
  - _coerce_to_existing_type: FA string → typed value, using existing
    runtime type OR codegen field_types.json as fallback
  - _validate_enum: rejects invalid enum values using codegen-emitted
    enum_values.json (single source of truth, not hand-mirrored)

Plus shape checks on the two codegen artifacts (enum_values.json +
field_types.json) so a future schema change that breaks the contract
is caught at build time, not at the FA's first failed edit.

CI runs this via `python3 -m unittest discover -s tests`. No Flask
process needed — we import the helpers directly. The full end-to-end
audit lives in scripts/audit_edit_paths.py and runs against a live
Flask + a real upload dir (manual / local).
"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

# Import without triggering Flask app startup — the module exposes
# the helpers as module-level functions.
from local_app_simple import (  # noqa: E402
    _ENUM_VALUES_BY_PATH,
    _FIELD_TYPES_BY_PATH,
    _coerce_to_existing_type,
    _fa_edit_meta,
    _normalize_path_template,
    _validate_enum,
    _walk_to_leaf,
)


def _wrapped(value):
    """Match the Rust Wrapped<T> JSON shape that the reducer emits."""
    return {"value": value, "_meta": {"confidence": "high", "evidence": [],
                                       "needs_review": False, "flags": []}}


class TestWalker(unittest.TestCase):
    def test_wrapped_leaf_returns_existing_and_setter_mutates(self):
        report = {"general_information": {"payee": {"name": _wrapped("Jane Doe")}}}
        result = _walk_to_leaf(report, "expense_report.general_information.payee.name")
        self.assertIsNotNone(result)
        existing, setter = result
        self.assertEqual(existing, "Jane Doe")
        setter("Edited Name")
        self.assertEqual(report["general_information"]["payee"]["name"]["value"], "Edited Name")

    def test_bare_scalar_leaf_returns_existing_and_setter_rebinds_on_parent(self):
        # authorized_by is Option<String> bare on the parent, no Wrapped.
        report = {"general_information": {"authorized_by": "advisor@stanford.edu"}}
        result = _walk_to_leaf(report, "expense_report.general_information.authorized_by")
        self.assertIsNotNone(result)
        existing, setter = result
        self.assertEqual(existing, "advisor@stanford.edu")
        setter("new@stanford.edu")
        self.assertEqual(report["general_information"]["authorized_by"], "new@stanford.edu")

    def test_list_index_returns_existing_and_setter_mutates(self):
        report = {"transaction_lines": [{"common": {"date": _wrapped("2024-09-02")}}]}
        result = _walk_to_leaf(report, "expense_report.transaction_lines[0].common.date")
        self.assertIsNotNone(result)
        existing, setter = result
        self.assertEqual(existing, "2024-09-02")
        setter("2024-09-03")
        self.assertEqual(report["transaction_lines"][0]["common"]["date"]["value"], "2024-09-03")

    def test_missing_last_key_auto_creates_wrapped(self):
        # The renderer emits a card for every schema leaf, but the codegen
        # skip-serializes empty Wrappeds — so the JSON may legitimately
        # not have the key. The walker should auto-create an empty Wrapped
        # on miss so the FA's edit lands.
        report = {"transaction_lines": [{"common": {}}]}
        result = _walk_to_leaf(report, "expense_report.transaction_lines[0].common.exchange_rate")
        self.assertIsNotNone(result)
        existing, setter = result
        self.assertIsNone(existing)
        setter(0.012)
        self.assertEqual(
            report["transaction_lines"][0]["common"]["exchange_rate"]["value"], 0.012
        )
        # The auto-created Wrapped carries fa_workbench_edit provenance.
        self.assertEqual(
            report["transaction_lines"][0]["common"]["exchange_rate"]["_meta"]["evidence"][0]["origin"],
            "fa_workbench_edit",
        )

    def test_missing_intermediate_key_fails(self):
        # Last-step miss is benign (auto-create); intermediate miss is a
        # real path error.
        report = {"general_information": {}}
        result = _walk_to_leaf(report, "expense_report.general_information.payee.name")
        self.assertIsNone(result)

    def test_out_of_range_list_index_fails(self):
        report = {"transaction_lines": [{}]}
        result = _walk_to_leaf(report, "expense_report.transaction_lines[5].common.date")
        self.assertIsNone(result)

    def test_garbage_path_fails(self):
        result = _walk_to_leaf({}, "not_a_real_path")
        self.assertIsNone(result)


class TestCoercion(unittest.TestCase):
    def test_bool_existing_yes_no(self):
        for yes_input in ("yes", "Yes", "true", "1", "y", "t"):
            self.assertEqual(_coerce_to_existing_type(yes_input, True),
                             (True, None), f"input: {yes_input!r}")
        for no_input in ("no", "False", "0", "n"):
            self.assertEqual(_coerce_to_existing_type(no_input, True),
                             (False, None), f"input: {no_input!r}")
        # Invalid bool string → error
        result, err = _coerce_to_existing_type("maybe", True)
        self.assertIsNone(result)
        self.assertIsNotNone(err)

    def test_number_existing_parses_clean_input(self):
        result, err = _coerce_to_existing_type("32.50", 100.0)
        self.assertEqual((result, err), (32.5, None))

    def test_number_existing_strips_currency_formatting(self):
        # FAs paste "$1,234.56" — we should accept it.
        result, err = _coerce_to_existing_type("$1,234.56", 100.0)
        self.assertEqual((result, err), (1234.56, None))

    def test_number_existing_rejects_garbage(self):
        result, err = _coerce_to_existing_type("abc", 100.0)
        self.assertIsNone(result)
        self.assertIn("number", err)

    def test_string_existing_passes_through(self):
        result, err = _coerce_to_existing_type("hello world", "old")
        self.assertEqual((result, err), ("hello world", None))

    def test_empty_string_clears_to_none(self):
        # On a string field, "" means clear.
        result, err = _coerce_to_existing_type("", "old")
        self.assertEqual((result, err), (None, None))

    def test_none_existing_falls_back_to_field_types_for_numeric(self):
        # Existing is None — runtime type unknown. Path lookup against
        # field_types.json tells us it's numeric → coerce to float.
        # Pick a real numeric path from the codegen output.
        numeric_paths = [
            p for p, meta in _FIELD_TYPES_BY_PATH.items()
            if meta.get("type") == "number"
        ]
        self.assertTrue(numeric_paths, "expected codegen to emit numeric field paths")
        path = numeric_paths[0]
        result, err = _coerce_to_existing_type("42.50", None, path)
        self.assertEqual((result, err), (42.5, None),
                         f"path {path!r} should coerce to number")

    def test_none_existing_falls_back_to_field_types_for_bool(self):
        bool_paths = [
            p for p, meta in _FIELD_TYPES_BY_PATH.items()
            if meta.get("type") == "boolean"
        ]
        self.assertTrue(bool_paths, "expected codegen to emit boolean field paths")
        path = bool_paths[0]
        result, err = _coerce_to_existing_type("yes", None, path)
        self.assertEqual((result, err), (True, None),
                         f"path {path!r} should coerce to bool")


class TestEnumValidation(unittest.TestCase):
    def test_valid_enum_value_accepted(self):
        # Pick a real enum path + value from the codegen-emitted SSOT.
        path = "expense_report.general_information.payee.affiliation"
        valid = _ENUM_VALUES_BY_PATH.get(path)
        self.assertTrue(valid, "expected affiliation enum in codegen output")
        # Build a concrete (non-template) path that should normalize to it.
        self.assertIsNone(_validate_enum(path, valid[0]))

    def test_invalid_enum_value_rejected_with_friendly_msg(self):
        path = "expense_report.general_information.payee.affiliation"
        err = _validate_enum(path, "stanford_professor_emeritus")
        self.assertIsNotNone(err)
        self.assertIn("must be one of", err)

    def test_non_enum_path_not_validated(self):
        # Path that exists in the schema but isn't an enum → no validation.
        self.assertIsNone(_validate_enum(
            "expense_report.general_information.payee.name",
            "anything goes here"
        ))

    def test_path_template_normalization(self):
        # transaction_lines[0] and transaction_lines[7] should both
        # resolve to the same enum entry.
        concrete_0 = "expense_report.transaction_lines[0].common.expense_type"
        concrete_7 = "expense_report.transaction_lines[7].common.expense_type"
        self.assertEqual(
            _normalize_path_template(concrete_0),
            _normalize_path_template(concrete_7),
        )
        # And the normalized form is the key we used in enum_values.json.
        template = _normalize_path_template(concrete_0)
        self.assertIn(template, _ENUM_VALUES_BY_PATH)


class TestCodegenArtifactShape(unittest.TestCase):
    """Guards against schema drift breaking the Stage 7 edit contract.

    If a schema change adds new enum values or new field types, these
    assertions stay green as long as the codegen + Python loader are
    in sync. If the JSON files vanish (codegen not run), they fail
    loudly — preventing a stale CI image from passing tests against
    out-of-date contracts.
    """

    def test_enum_values_json_loaded(self):
        # Codegen must have run + Python must have loaded the file.
        self.assertGreater(len(_ENUM_VALUES_BY_PATH), 0,
                           "generated/enum_values.json missing or empty — run "
                           "scripts/generate_schema_artifacts.py")

    def test_field_types_json_loaded(self):
        self.assertGreater(len(_FIELD_TYPES_BY_PATH), 0,
                           "generated/field_types.json missing or empty — run "
                           "scripts/generate_schema_artifacts.py")

    def test_enum_values_files_have_lists_of_strings(self):
        for path, values in _ENUM_VALUES_BY_PATH.items():
            self.assertIsInstance(values, list, f"path: {path!r}")
            self.assertGreater(len(values), 0, f"path: {path!r}")
            for v in values:
                self.assertIsInstance(v, str, f"path: {path!r}, value: {v!r}")

    def test_field_types_have_type_field(self):
        valid_types = {"string", "number", "boolean", "enum", "date"}
        for path, meta in _FIELD_TYPES_BY_PATH.items():
            self.assertIsInstance(meta, dict, f"path: {path!r}")
            self.assertIn("type", meta, f"path: {path!r}")
            self.assertIn(meta["type"], valid_types,
                          f"path: {path!r} has unknown type {meta['type']!r}")

    def test_enum_paths_are_also_in_field_types(self):
        # If a path is in enum_values, it must also appear in field_types
        # with type='enum'. Catches codegen bugs where the two are out of sync.
        for path in _ENUM_VALUES_BY_PATH:
            self.assertIn(path, _FIELD_TYPES_BY_PATH,
                          f"enum path {path!r} missing from field_types")
            self.assertEqual(_FIELD_TYPES_BY_PATH[path]["type"], "enum",
                             f"enum path {path!r} has wrong type in field_types")


class TestFaEditMeta(unittest.TestCase):
    def test_shape_matches_rust_field_metadata(self):
        meta = _fa_edit_meta()
        self.assertEqual(meta["confidence"], "high")
        self.assertIsInstance(meta["evidence"], list)
        self.assertEqual(meta["evidence"][0]["kind"], "user_input")
        self.assertEqual(meta["evidence"][0]["origin"], "fa_workbench_edit")
        self.assertFalse(meta["needs_review"])
        self.assertEqual(meta["flags"], [])


if __name__ == "__main__":
    unittest.main()
