"""Unit tests for scripts/extractor_lib.py's retry helper.

Validates the retry classification + backoff timing without making
real Gemini calls. Runs in <5 seconds total.
"""

from __future__ import annotations

import sys
import time
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from extractor_lib import (  # noqa: E402
    RETRYABLE_HTTP_STATUSES,
    RETRY_MAX_ATTEMPTS,
    _is_retryable,
    _retry_with_backoff,
    merge_two_call_lines,
)


def _err_with_code(code: int) -> Exception:
    """Synthesize an exception with a `.code` attribute, mimicking
    google.genai.errors.ClientError's shape."""
    e = Exception(f"synthetic HTTP {code}")
    e.code = code
    return e


class TestIsRetryable(unittest.TestCase):
    def test_5xx_retryable(self):
        for code in (500, 502, 503, 504):
            self.assertTrue(_is_retryable(_err_with_code(code)),
                            f"HTTP {code} should be retryable")

    def test_429_and_408_retryable(self):
        for code in (408, 429):
            self.assertTrue(_is_retryable(_err_with_code(code)))

    def test_4xx_not_retryable(self):
        # 400 INVALID_ARGUMENT (the invalid-argument class) MUST NOT
        # retry — retrying gives the same answer and wastes quota.
        for code in (400, 401, 403, 404):
            self.assertFalse(_is_retryable(_err_with_code(code)),
                             f"HTTP {code} should NOT be retryable")

    def test_network_errors_retryable(self):
        self.assertTrue(_is_retryable(TimeoutError("timed out")))
        self.assertTrue(_is_retryable(ConnectionError("connection reset")))

    def test_value_error_not_retryable(self):
        # JSONDecodeError + ValueError are not transient.
        self.assertFalse(_is_retryable(ValueError("bad json")))

    def test_google_api_core_classes_retryable_by_name(self):
        # We detect google.api_core's transient classes by name to avoid
        # importing google.api_core just for this check.
        for name in ("ServiceUnavailable", "InternalServerError",
                     "DeadlineExceeded", "GoogleAPIError", "RetryError"):
            cls = type(name, (Exception,), {})
            self.assertTrue(_is_retryable(cls()),
                            f"{name} should be retryable by class-name match")


class TestRetryWithBackoff(unittest.TestCase):
    def test_first_call_succeeds_no_retry(self):
        calls = []

        def fn():
            calls.append(1)
            return "ok"

        self.assertEqual(_retry_with_backoff(fn, label="x"), "ok")
        self.assertEqual(len(calls), 1)

    def test_retries_until_success(self):
        calls = []

        def fn():
            calls.append(1)
            if len(calls) < 3:
                raise _err_with_code(503)
            return "recovered"

        start = time.time()
        result = _retry_with_backoff(fn, label="x")
        elapsed = time.time() - start
        self.assertEqual(result, "recovered")
        self.assertEqual(len(calls), 3)
        # Backoffs 1s + 2s = 3s minimum. Loose upper bound for CI jitter.
        self.assertGreater(elapsed, 2.5)
        self.assertLess(elapsed, 5.0)

    def test_exhausts_attempts_and_raises_last(self):
        attempts = []

        def fn():
            attempts.append(1)
            raise _err_with_code(503)

        with self.assertRaises(Exception) as ctx:
            _retry_with_backoff(fn, label="x")
        self.assertEqual(getattr(ctx.exception, "code", None), 503)
        self.assertEqual(len(attempts), RETRY_MAX_ATTEMPTS)

    def test_non_retryable_raises_immediately(self):
        attempts = []

        def fn():
            attempts.append(1)
            raise _err_with_code(400)

        with self.assertRaises(Exception):
            _retry_with_backoff(fn, label="x")
        # No retries — 400 is permanent.
        self.assertEqual(len(attempts), 1)

    def test_retryable_statuses_constant_shape(self):
        # Guard against accidental tightening of the retryable set.
        # If we remove 503, we lose retry on the most common Vertex flake.
        for code in (408, 429, 500, 502, 503, 504):
            self.assertIn(code, RETRYABLE_HTTP_STATUSES)


class TestMergeTwoCallLines(unittest.TestCase):
    """B1.r: covers the merge helper that lodging+meal+transport
    all use to combine their two parallel single_call outputs into
    one per-doc transaction line."""

    def test_disjoint_keys_merge_into_one_line(self):
        main = [{"common": {"x": 1}, "meal_details": {"venue": "Tamarine"}}]
        extras = [{"extras": {"merchant_address": "123 Main St"}}]
        merged = merge_two_call_lines(main, extras)
        self.assertEqual(len(merged), 1)
        self.assertEqual(set(merged[0].keys()),
                         {"common", "meal_details", "extras"})
        self.assertEqual(merged[0]["meal_details"]["venue"], "Tamarine")
        self.assertEqual(merged[0]["extras"]["merchant_address"], "123 Main St")

    def test_main_not_single_element_raises(self):
        with self.assertRaises(ValueError):
            merge_two_call_lines([], [{"extras": {}}])
        with self.assertRaises(ValueError):
            merge_two_call_lines([{"a": 1}, {"b": 2}], [{"extras": {}}])

    def test_extras_not_single_element_raises(self):
        with self.assertRaises(ValueError):
            merge_two_call_lines([{"common": {}}], [])

    def test_non_dict_elements_raise(self):
        with self.assertRaises(ValueError):
            merge_two_call_lines(["not a dict"], [{"extras": {}}])
        with self.assertRaises(ValueError):
            merge_two_call_lines([{"common": {}}], ["not a dict"])

    def test_extras_keys_win_on_conflict(self):
        # Defensive: if both calls accidentally emit the same key,
        # the spread order means extras wins. This shouldn't happen
        # given disjoint schemas, but the merge shouldn't crash.
        main = [{"common": {"v": "main"}}]
        extras = [{"common": {"v": "extras"}, "extras": {}}]
        merged = merge_two_call_lines(main, extras)
        self.assertEqual(merged[0]["common"]["v"], "extras")


if __name__ == "__main__":
    unittest.main()
