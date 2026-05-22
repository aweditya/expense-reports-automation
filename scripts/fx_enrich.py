"""Pipeline step: enrich `reduced/report.json` with real FX rates.

Runs between `reduce_extractions` and `render_workbench_from_report`.
For every foreign-currency transaction line (has `common.original_
currency` + `common.original_amount`), fetches the historical rate
from Frankfurter (via `fx_lookup.fetch_rate`), overwrites
`common.exchange_rate.value` + `common.line_amount_usd.value` (+
their `_meta.evidence` to point at the Frankfurter call), then
cascades `transaction_summary.total_usd`.

Failure-tolerant — if Frankfurter is unreachable or the currency
isn't covered, the line keeps whatever the reducer's mock FX
produced, and a warning is printed. The workbench will still render;
the FA can correct the rate via the click-to-edit flow.

Usage:
  scripts/fx_enrich.py --in <path-to-report.json> --out <same or different>

Idempotent: re-running with the same input gives the same output
(modulo Frankfurter changing historical rates, which it doesn't).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from fx_lookup import fetch_rate  # noqa: E402


def fa_fx_meta() -> dict:
    """Field metadata stamp for rates we computed from Frankfurter."""
    return {
        "confidence": "high",
        "evidence": [{
            "kind": "system_generated",
            "filename": None,
            "page": None,
            "bboxes": None,
            "quote": None,
            "origin": "fx_enrich.frankfurter",
            "url": None,
            "token_ids": None,
        }],
        "needs_review": False,
        "flags": [],
        "confidence_reason": None,
    }


def enrich_report(report: dict) -> tuple[int, int, int]:
    """Walk the report in-place. Returns (lines_fetched, lines_skipped,
    lines_failed)."""
    lines = report.get("transaction_lines") or []
    cache: dict[tuple[str, str], float | None] = {}
    fetched = 0
    skipped = 0
    failed = 0

    for line in lines:
        common = line.get("common") or {}
        ccy_wrap = common.get("original_currency") or {}
        amt_wrap = common.get("original_amount") or {}
        date_wrap = common.get("date") or {}
        ccy = (ccy_wrap.get("value") or "").strip()
        amt = amt_wrap.get("value")
        date = (date_wrap.get("value") or "").strip()

        if not ccy or amt is None or not date:
            skipped += 1
            continue

        rate = fetch_rate(ccy, date, cache=cache)
        if rate is None:
            failed += 1
            continue

        usd = round(float(amt) * float(rate), 2)
        common["exchange_rate"] = {"value": rate, "_meta": fa_fx_meta()}
        common["line_amount_usd"] = {"value": usd, "_meta": fa_fx_meta()}
        fetched += 1

    # Recompute transaction_summary.total_usd as the sum of all lines'
    # line_amount_usd (whether enriched or mock/extracted).
    total = 0.0
    for line in lines:
        v = (line.get("common") or {}).get("line_amount_usd", {}).get("value")
        if isinstance(v, (int, float)):
            total += float(v)
    summary = report.setdefault("transaction_summary", {})
    cur_total = summary.get("total_usd") or {}
    cur_total["value"] = round(total, 2)
    # Preserve existing _meta if present; otherwise stamp as system-generated.
    cur_total.setdefault("_meta", fa_fx_meta())
    summary["total_usd"] = cur_total

    return fetched, skipped, failed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--in", dest="in_path", required=True, type=Path)
    parser.add_argument("--out", dest="out_path", required=True, type=Path)
    args = parser.parse_args()

    if not args.in_path.exists():
        print(f"error: {args.in_path} does not exist", file=sys.stderr)
        return 2

    report = json.loads(args.in_path.read_text())
    fetched, skipped, failed = enrich_report(report)
    args.out_path.parent.mkdir(parents=True, exist_ok=True)
    args.out_path.write_text(json.dumps(report, indent=2, ensure_ascii=False))
    print(f"# fx_enrich: {fetched} lines enriched, {skipped} skipped (USD/missing), {failed} failed",
          file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
