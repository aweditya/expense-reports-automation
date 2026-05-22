"""Tests for scripts/fx_lookup.py — Frankfurter HTTP wrapper with cache.

Network calls are mocked via monkey-patched urlopen — the test suite
runs without internet (CI / Cloud Build). One live-network smoke test
is included but skipped by default; set FX_LIVE_TEST=1 to enable.
"""

from __future__ import annotations

import io
import json
import os
import sys
import unittest
import unittest.mock
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from fx_lookup import fetch_rate  # noqa: E402


def fake_response(body: dict, status: int = 200):
    """Return a context-manager mock that mimics urllib.request.urlopen's
    return value (`.read()` + `.getcode()`)."""
    cm = unittest.mock.MagicMock()
    cm.__enter__.return_value.read.return_value = json.dumps(body).encode("utf-8")
    cm.__enter__.return_value.getcode.return_value = status
    return cm


class TestFetchRate(unittest.TestCase):
    def test_usd_returns_one_without_fetch(self):
        # No mock needed — fetch_rate short-circuits USD.
        self.assertEqual(fetch_rate("USD", "2024-09-02"), 1.0)
        self.assertEqual(fetch_rate("usd", "2024-09-02"), 1.0)

    def test_empty_currency_or_date_returns_none(self):
        self.assertIsNone(fetch_rate("", "2024-09-02"))
        self.assertIsNone(fetch_rate("INR", ""))

    @unittest.mock.patch("fx_lookup.urllib.request.urlopen")
    def test_parses_frankfurter_response(self, mock_open):
        mock_open.return_value = fake_response({
            "amount": 1.0, "base": "INR", "date": "2024-09-02",
            "rates": {"USD": 0.01192},
        })
        self.assertEqual(fetch_rate("INR", "2024-09-02"), 0.01192)
        # Confirm the URL we hit looked right.
        called_url = mock_open.call_args[0][0].full_url
        self.assertIn("/2024-09-02", called_url)
        self.assertIn("from=INR", called_url)
        self.assertIn("to=USD", called_url)

    @unittest.mock.patch("fx_lookup.urllib.request.urlopen")
    def test_cache_hit_skips_fetch(self, mock_open):
        cache: dict = {("INR", "2024-09-02"): 0.012}
        result = fetch_rate("INR", "2024-09-02", cache=cache)
        self.assertEqual(result, 0.012)
        mock_open.assert_not_called()

    @unittest.mock.patch("fx_lookup.urllib.request.urlopen")
    def test_cache_miss_populates_after_fetch(self, mock_open):
        mock_open.return_value = fake_response({
            "amount": 1.0, "base": "CHF", "date": "2024-05-01",
            "rates": {"USD": 1.10},
        })
        cache: dict = {}
        fetch_rate("CHF", "2024-05-01", cache=cache)
        self.assertEqual(cache.get(("CHF", "2024-05-01")), 1.10)

    @unittest.mock.patch("fx_lookup.urllib.request.urlopen")
    def test_http_error_caches_none(self, mock_open):
        import urllib.error
        mock_open.side_effect = urllib.error.HTTPError(
            "url", 422, "Unprocessable Entity", {}, io.BytesIO(b"{}")
        )
        cache: dict = {}
        result = fetch_rate("ZZX", "2024-09-02", cache=cache)
        self.assertIsNone(result)
        # Negative cache: we should NOT retry on the same key.
        self.assertIn(("ZZX", "2024-09-02"), cache)
        self.assertIsNone(cache[("ZZX", "2024-09-02")])

    @unittest.mock.patch("fx_lookup.urllib.request.urlopen")
    def test_network_error_does_not_cache(self, mock_open):
        import urllib.error
        mock_open.side_effect = urllib.error.URLError("network down")
        cache: dict = {}
        result = fetch_rate("INR", "2024-09-02", cache=cache)
        self.assertIsNone(result)
        # No negative cache for transient network failures — caller may
        # retry on the next pipeline run when network is back.
        self.assertNotIn(("INR", "2024-09-02"), cache)

    @unittest.mock.patch("fx_lookup.urllib.request.urlopen")
    def test_bad_json_returns_none(self, mock_open):
        cm = unittest.mock.MagicMock()
        cm.__enter__.return_value.read.return_value = b"<html>oops</html>"
        cm.__enter__.return_value.getcode.return_value = 200
        mock_open.return_value = cm
        self.assertIsNone(fetch_rate("INR", "2024-09-02"))

    @unittest.mock.patch("fx_lookup.urllib.request.urlopen")
    def test_zero_or_missing_usd_rate_returns_none(self, mock_open):
        mock_open.return_value = fake_response({
            "amount": 1.0, "base": "INR", "date": "2024-09-02",
            "rates": {"EUR": 0.011},  # no USD!
        })
        self.assertIsNone(fetch_rate("INR", "2024-09-02"))


@unittest.skipUnless(os.environ.get("FX_LIVE_TEST"), "live HTTP, opt-in")
class TestFetchRateLive(unittest.TestCase):
    def test_real_frankfurter_call_returns_plausible_inr_rate(self):
        # Sept 2024 INR/USD was ~0.012. Allow a wide tolerance.
        rate = fetch_rate("INR", "2024-09-02")
        self.assertIsNotNone(rate)
        self.assertTrue(0.005 < rate < 0.025, f"unexpected rate: {rate}")


if __name__ == "__main__":
    unittest.main()
