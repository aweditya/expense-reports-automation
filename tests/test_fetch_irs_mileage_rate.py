"""Unit tests for scripts/fetch_irs_mileage_rate.py (B2).

The parser is the part most likely to break — IRS doesn't promise
stable HTML structure. These tests pin the parser's behavior against
a frozen-in-time HTML snippet that mirrors what we saw on
2026-05-23: a clean <table class="complex-table"> with header row +
yearly + mid-year-split rows.

If the IRS page layout changes, scripts/fetch_irs_mileage_rate.py
will still detect the failure (parse() raises ValueError, main()
prints to stderr, the committed JSON cache stays untouched), but
these tests prove the parser handles the SHAPE we know works.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from fetch_irs_mileage_rate import (  # noqa: E402
    MileageRatesParser,
    build_output,
    parse,
)


# Frozen snippet representative of the actual IRS page on 2026-05-23.
# Trimmed to the table we care about + minimal surrounding markup.
IRS_HTML_SAMPLE = """
<html><body>
<p>Some intro text that should be ignored.</p>
<table class="table other-table">
  <thead><tr><th>Decoy</th></tr></thead>
  <tbody><tr><td>this row must NOT be in our output</td></tr></tbody>
</table>
<h2>Mileage rates for all years (cents/mile)</h2>
<table class="table complex-table table-striped table-bordered table-responsive">
  <thead>
    <tr>
      <th>Period</th>
      <th>Business use</th>
      <th>Charity use</th>
      <th>Medical or military moving</th>
      <th>Source</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>2025</td>
      <td>70</td>
      <td>14</td>
      <td>21</td>
      <td><a href="/x">IR-2024-312</a></td>
    </tr>
    <tr>
      <td>2024</td>
      <td>67</td>
      <td>14</td>
      <td>21</td>
      <td><a href="/y">IR-2023-239</a></td>
    </tr>
    <tr>
      <td>7/1/2022-12/31/2022</td>
      <td>62.5</td>
      <td>14</td>
      <td>22</td>
      <td><a href="/z">IR-2022-mid</a></td>
    </tr>
  </tbody>
</table>
</body></html>
"""


class TestMileageRatesParser(unittest.TestCase):
    def test_parses_three_rows_from_target_table(self):
        rates = parse(IRS_HTML_SAMPLE)
        self.assertEqual(len(rates), 3,
                         "should parse exactly 3 rows from the target table")

    def test_skips_decoy_table_with_different_class(self):
        """The IRS page has multiple <table> elements; only the one
        with class='complex-table' is the rates table. Make sure
        we don't pick up rows from any other table."""
        rates = parse(IRS_HTML_SAMPLE)
        # Decoy row's first cell was "this row must NOT be in our output"
        # — if it leaked through, it'd appear as a period name.
        for period in rates:
            self.assertNotIn("must NOT", period)

    def test_current_2025_business_rate(self):
        rates = parse(IRS_HTML_SAMPLE)
        self.assertIn("2025", rates)
        self.assertEqual(rates["2025"]["business"], 70.0)
        self.assertEqual(rates["2025"]["charity"], 14.0)
        self.assertEqual(rates["2025"]["medical_or_military_moving"], 21.0)

    def test_preserves_mid_year_split_period(self):
        """IRS occasionally splits a year (2022 had a mid-year fuel-
        price adjustment). The period key should be the raw string
        as printed — reduction interprets it."""
        rates = parse(IRS_HTML_SAMPLE)
        self.assertIn("7/1/2022-12/31/2022", rates)
        self.assertEqual(
            rates["7/1/2022-12/31/2022"]["business"], 62.5)

    def test_2023_floating_point_rate(self):
        """65.5 was the 2023 business rate — confirms we don't drop
        the fractional part on float() conversion."""
        # Add a 2023 row inline to test float handling.
        html_with_2023 = IRS_HTML_SAMPLE.replace(
            "<td>2024</td>",
            "<td>2023</td><td>65.5</td><td>14</td><td>22</td><td><a>x</a></td></tr><tr><td>2024</td>",
        )
        rates = parse(html_with_2023)
        self.assertEqual(rates["2023"]["business"], 65.5)

    def test_empty_html_raises_value_error(self):
        with self.assertRaises(ValueError):
            parse("<html><body><p>no table</p></body></html>")

    def test_table_without_rows_raises_value_error(self):
        html = """
        <table class="complex-table">
          <thead><tr><th>Period</th></tr></thead>
          <tbody></tbody>
        </table>"""
        with self.assertRaises(ValueError):
            parse(html)

    def test_table_with_non_numeric_cells_skips_those_rows(self):
        """A row with garbage data shouldn't crash the parser, just
        be dropped silently. If ALL rows are garbage, parse should
        still raise (no usable data)."""
        html_with_garbage = IRS_HTML_SAMPLE.replace(
            "<td>2024</td>\n      <td>67</td>",
            "<td>2024</td>\n      <td>not-a-number</td>",
        )
        rates = parse(html_with_garbage)
        # 2024 row dropped (garbage), 2025 + mid-year-2022 still there.
        self.assertEqual(len(rates), 2)
        self.assertIn("2025", rates)
        self.assertNotIn("2024", rates)


class TestBuildOutput(unittest.TestCase):
    def test_envelope_has_provenance_fields(self):
        rates = parse(IRS_HTML_SAMPLE)
        env = build_output(rates)
        self.assertIn("fetched_at", env)
        self.assertIn("source_url", env)
        self.assertEqual(env["unit"], "cents_per_mile")
        self.assertEqual(env["rates_by_period"], rates)

    def test_envelope_fetched_at_is_iso8601_utc(self):
        env = build_output(parse(IRS_HTML_SAMPLE))
        # Should end in "+00:00" (timezone-aware UTC isoformat).
        self.assertTrue(env["fetched_at"].endswith("+00:00"),
                        f"fetched_at should be UTC: {env['fetched_at']}")


if __name__ == "__main__":
    unittest.main()
