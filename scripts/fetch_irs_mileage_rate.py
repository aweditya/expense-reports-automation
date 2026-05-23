#!/usr/bin/env python3
"""Fetch the IRS standard mileage rates table → generated/irs_mileage_rates.json.

Manual codegen-style fetch (B2): run this script once per year (or
whenever you see a news article about IRS rate changes) to refresh
the rates the pipeline uses for personal mileage reimbursements.
The output JSON is committed to git so:
  1. Reduction has no runtime dependency on IRS being reachable.
  2. Rate changes show up in a commit (auditable + reviewable).
  3. The pipeline never silently uses a wrong rate.

Source page:
  https://www.irs.gov/tax-professionals/standard-mileage-rates

The page has a clean HTML table:
  <table class="table complex-table table-striped table-bordered table-responsive">
    <thead> Period | Business use | Charity use | Medical or military moving | Source </thead>
    <tbody> <tr> <td>2025</td> <td>70</td> <td>14</td> <td>21</td> ... </tr> ...
The values are cents per mile. We parse the rows, normalize the
period (year or YYYY-mid-year-split), and write JSON.

Fall-back behavior on fetch / parse failure: print the error and
exit non-zero. The committed JSON survives unchanged. Don't
silently substitute a guess; an outdated-but-correct rate is fine,
a wrong rate is not.

Usage:
  ./.venv/bin/python scripts/fetch_irs_mileage_rate.py
  ./.venv/bin/python scripts/fetch_irs_mileage_rate.py --out path/to/different.json
  ./.venv/bin/python scripts/fetch_irs_mileage_rate.py --print  # dry-run, print to stdout
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
import urllib.error
import urllib.request
from html.parser import HTMLParser
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUT = REPO_ROOT / "generated" / "irs_mileage_rates.json"
SOURCE_URL = "https://www.irs.gov/tax-professionals/standard-mileage-rates"

# Same UA pattern Stage 9b uses for Frankfurter (some CDNs reject the
# default Python-urllib UA). IRS doesn't seem to block urllib today,
# but explicit UA is safer + identifies us in their logs.
USER_AGENT = (
    "stanford-expense-reports/1.0 "
    "(+https://expense-reports-wgnivgelea-uw.a.run.app)"
)


class MileageRatesParser(HTMLParser):
    """Extracts the IRS mileage rates table rows.

    The table we want is identified by the unique class attribute
    `complex-table` (IRS Drupal convention; other tables on the
    page lack this class). We only capture <tr> rows inside the
    target table's <tbody>; the <thead> + non-target tables get
    skipped. Cell text is concatenated lowercase-tag-free.
    """

    TARGET_TABLE_CLASS = "complex-table"

    def __init__(self):
        super().__init__()
        self._in_target_table = False
        self._in_target_tbody = False
        self._in_tr = False
        self._in_td = False
        self._current_cell_chars: list[str] = []
        self._current_row: list[str] = []
        self.rows: list[list[str]] = []

    def handle_starttag(self, tag, attrs):
        attrs_d = dict(attrs)
        if tag == "table" and self.TARGET_TABLE_CLASS in (attrs_d.get("class") or ""):
            self._in_target_table = True
        elif tag == "tbody" and self._in_target_table:
            self._in_target_tbody = True
        elif tag == "tr" and self._in_target_tbody:
            self._in_tr = True
            self._current_row = []
        elif tag == "td" and self._in_tr:
            self._in_td = True
            self._current_cell_chars = []

    def handle_endtag(self, tag):
        if tag == "td" and self._in_td:
            self._current_row.append("".join(self._current_cell_chars).strip())
            self._in_td = False
        elif tag == "tr" and self._in_tr:
            if self._current_row:
                self.rows.append(self._current_row)
            self._in_tr = False
        elif tag == "tbody" and self._in_target_tbody:
            self._in_target_tbody = False
        elif tag == "table" and self._in_target_table:
            self._in_target_table = False

    def handle_data(self, data):
        if self._in_td:
            self._current_cell_chars.append(data)


def fetch(url: str = SOURCE_URL) -> str:
    """GET the IRS rates page. Raises urllib.error.URLError on failure
    (caller decides what to do — typically exit non-zero so a stale
    but correct JSON file stays untouched in the repo)."""
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=15) as resp:
        if resp.status != 200:
            raise urllib.error.URLError(
                f"IRS returned HTTP {resp.status}"
            )
        return resp.read().decode("utf-8")


def parse(html: str) -> dict:
    """Parse IRS HTML into a {year/period → {business, charity,
    medical_or_military_moving}} map. Values are CENTS per mile
    (as printed in the IRS table); reduction converts to dollars
    at apply time.

    Raises ValueError if the table can't be found or has fewer
    than expected columns — better to fail loudly than write a
    half-populated cache file."""
    parser = MileageRatesParser()
    parser.feed(html)
    if not parser.rows:
        raise ValueError(
            f"no rows found in IRS table on {SOURCE_URL} — "
            "did the page layout change?"
        )
    rates: dict[str, dict[str, float]] = {}
    for row in parser.rows:
        # Row shape: [period, business, charity, medical, source-link-text]
        # We tolerate the source cell being absent / non-numeric.
        if len(row) < 4:
            continue
        period, business, charity, medical = row[0], row[1], row[2], row[3]
        try:
            rates[period] = {
                "business": float(business),
                "charity": float(charity),
                "medical_or_military_moving": float(medical),
            }
        except ValueError:
            # Skip rows whose numbers aren't parseable (e.g. a footnote
            # row or rendering quirk). Don't poison the cache.
            continue
    if not rates:
        raise ValueError(
            "parsed zero numeric rate rows — page format may have changed"
        )
    return rates


def build_output(rates: dict) -> dict:
    """JSON envelope: rates + provenance (so a future reader can
    see when the cache was last refreshed and from where)."""
    return {
        "$comment": (
            "Cached IRS standard mileage rates. Regenerate by running "
            "scripts/fetch_irs_mileage_rate.py. Values are cents/mile."
        ),
        "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
        "source_url": SOURCE_URL,
        "unit": "cents_per_mile",
        "rates_by_period": rates,
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out", type=Path, default=DEFAULT_OUT,
                   help=f"output path (default: {DEFAULT_OUT})")
    p.add_argument("--print", action="store_true",
                   help="print the parsed JSON to stdout instead of writing")
    args = p.parse_args()

    try:
        html = fetch()
    except (urllib.error.URLError, TimeoutError) as e:
        print(f"fetch failed: {e}", file=sys.stderr)
        print("kept existing cache unchanged.", file=sys.stderr)
        return 1
    try:
        rates = parse(html)
    except ValueError as e:
        print(f"parse failed: {e}", file=sys.stderr)
        print("kept existing cache unchanged.", file=sys.stderr)
        return 2

    output = build_output(rates)
    if args.print:
        print(json.dumps(output, indent=2))
        return 0

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(output, indent=2) + "\n")
    n_periods = len(rates)
    latest = next(iter(rates))  # rates dict preserves insertion order
    print(
        f"wrote {args.out} ({n_periods} periods; latest='{latest}' "
        f"business={rates[latest]['business']}¢/mi)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
