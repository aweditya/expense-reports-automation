"""Unit tests for the per-upload edit history helpers (Stage 23).

Covers the four primitives in scripts/local_app_simple.py:
  _read_history / _write_history / _append_history /
  _pop_history / _clear_history

Doesn't exercise the Flask endpoints — those have end-to-end
eyeball coverage via curl. These tests guard the on-disk shape +
the FIFO cap.
"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from local_app_simple import (  # noqa: E402
    EDIT_HISTORY_MAX_ENTRIES,
    _append_history,
    _clear_history,
    _history_path,
    _pop_history,
    _read_history,
    _write_history,
)


class TestEditHistory(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.upload_dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_read_history_returns_empty_when_missing(self):
        self.assertEqual(_read_history(self.upload_dir), [])

    def test_append_then_read_returns_one_entry(self):
        _append_history(self.upload_dir, "expense_report.x", "old", "new")
        h = _read_history(self.upload_dir)
        self.assertEqual(len(h), 1)
        self.assertEqual(h[0]["path"], "expense_report.x")
        self.assertEqual(h[0]["old_value"], "old")
        self.assertEqual(h[0]["new_value"], "new")
        self.assertIn("timestamp", h[0])

    def test_pop_returns_latest_and_removes(self):
        _append_history(self.upload_dir, "x", 1, 2)
        _append_history(self.upload_dir, "y", 3, 4)
        popped = _pop_history(self.upload_dir)
        self.assertEqual(popped["path"], "y")
        # After pop, only the first entry remains.
        h = _read_history(self.upload_dir)
        self.assertEqual(len(h), 1)
        self.assertEqual(h[0]["path"], "x")

    def test_pop_empty_returns_none(self):
        self.assertIsNone(_pop_history(self.upload_dir))

    def test_clear_removes_file(self):
        _append_history(self.upload_dir, "x", 1, 2)
        self.assertTrue(_history_path(self.upload_dir).exists())
        _clear_history(self.upload_dir)
        self.assertFalse(_history_path(self.upload_dir).exists())
        # And subsequent read returns empty (not error).
        self.assertEqual(_read_history(self.upload_dir), [])

    def test_clear_when_already_missing_is_noop(self):
        # Should not error.
        _clear_history(self.upload_dir)
        self.assertFalse(_history_path(self.upload_dir).exists())

    def test_fifo_cap_drops_oldest(self):
        for i in range(EDIT_HISTORY_MAX_ENTRIES + 5):
            _append_history(self.upload_dir, f"path.{i}", i, i + 1)
        h = _read_history(self.upload_dir)
        self.assertEqual(len(h), EDIT_HISTORY_MAX_ENTRIES,
                         "history must be capped at the limit")
        # Oldest 5 should have been dropped; remaining starts at index 5.
        self.assertEqual(h[0]["path"], "path.5")
        self.assertEqual(h[-1]["path"],
                         f"path.{EDIT_HISTORY_MAX_ENTRIES + 4}")

    def test_corrupted_history_file_returns_empty(self):
        _history_path(self.upload_dir).write_text("not valid json {")
        # Don't crash; just treat as empty.
        self.assertEqual(_read_history(self.upload_dir), [])

    def test_history_with_non_list_root_returns_empty(self):
        _history_path(self.upload_dir).write_text('{"oops": "dict"}')
        self.assertEqual(_read_history(self.upload_dir), [])

    def test_appended_entries_preserve_order(self):
        for i in range(5):
            _append_history(self.upload_dir, f"p{i}", None, i)
        h = _read_history(self.upload_dir)
        self.assertEqual([entry["path"] for entry in h],
                         ["p0", "p1", "p2", "p3", "p4"])


if __name__ == "__main__":
    unittest.main()
