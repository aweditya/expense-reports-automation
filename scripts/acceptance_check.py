#!/usr/bin/env python3
"""Acceptance check for the per-kind extractors on real receipts.

By default reads the JSON files already in `.scratch/spike/`. With `--run`,
re-invokes the extractor against the source images first. A real run costs
one Gemini call per receipt (~30s each) so this is a manual sanity
script, not a CI test.

Per-receipt assertions are hand-coded based on observed extractor output.
They cover the values where being wrong matters (date, total, venue /
origin / destination, expense kind, alcohol presence). After the Phase-1
enum collapse `expense_type` is just `business_meal` for every meal
receipt — alcohol presence is carried by the separate
`has_alcohol_on_receipt` flag.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
PYTHON = REPO_ROOT / ".venv" / "bin" / "python"

# Per-kind extractor scripts. The acceptance harness routes each entry
# to the right one via its `extractor` field.
EXTRACTORS = {
    "meal": REPO_ROOT / "scripts" / "extract_meal.py",
    "transport": REPO_ROOT / "scripts" / "extract_transport.py",
    "lodging": REPO_ROOT / "scripts" / "extract_lodging.py",
}


# Each entry: image, output, extractor (which kind), expectations.
# Expectations: dict where keys are dotted paths into the transaction line
# (e.g., "common.date.value"), values are either a literal (strict equality)
# or a callable taking the actual value and returning True/False.
RECEIPTS = [
    # ─── Meals ────────────────────────────────────────────────────────────
    # NOTE: file names here reference the older corpus (mels1.jpeg etc.);
    # the actual receipts/ directory now has mels.jpeg + mjsushi1.jpeg +
    # mjsushi2.jpeg. Without `--run`, this still works against the cached
    # .scratch/spike/*.json files. With `--run`, these entries will fail
    # on missing files until the corpus references are refreshed (separate
    # cleanup; out of scope for the Phase 2 Stage 1b commit that added
    # the transport entries below).
    {
        "image": "receipts/mels1.jpeg",
        "output": ".scratch/spike/mels1.json",
        "extractor": "meal",
        "expect": {
            "common.date.value": "2026-04-19",
            "common.line_amount_usd.value": 163.54,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal",
            "meal_details.venue_name.value": lambda v: v
            and "MJ Sushi" in v,
            "meal_details.has_alcohol_on_receipt.value": True,
            "extras.printed_currency.value": "USD",
            "extras.merchant_address.value": lambda v: v and "Palo Alto" in v,
        },
    },
    {
        "image": "receipts/mels2.jpeg",
        "output": ".scratch/spike/mels2.json",
        "extractor": "meal",
        "expect": {
            "common.date.value": "2026-04-04",
            "common.line_amount_usd.value": 123.19,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal",
            "meal_details.venue_name.value": lambda v: v
            and "Original Mels" in v,
            "meal_details.has_alcohol_on_receipt.value": False,
            "extras.printed_currency.value": "USD",
            "extras.merchant_address.value": lambda v: v and "San Leandro" in v,
        },
    },
    {
        "image": "receipts/tamarine.png",
        "output": ".scratch/spike/tamarine.json",
        "extractor": "meal",
        "expect": {
            "common.date.value": "2026-03-05",
            "common.line_amount_usd.value": 387.12,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal",
            "meal_details.venue_name.value": lambda v: v
            and "Tamarine" in v,
            "meal_details.has_alcohol_on_receipt.value": True,
            "extras.printed_currency.value": "USD",
            "extras.merchant_address.value": lambda v: v and "Palo Alto" in v,
        },
    },
    {
        "image": "receipts/mjsushi.jpeg",
        "output": ".scratch/spike/mjsushi.json",
        "extractor": "meal",
        "expect": {
            "common.date.value": "2026-05-02",
            "common.line_amount_usd.value": 79.59,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal",
            "meal_details.venue_name.value": lambda v: v and "MJ Sushi" in v,
            "meal_details.has_alcohol_on_receipt.value": True,
            "extras.printed_currency.value": "USD",
            "extras.merchant_address.value": lambda v: v and "Palo Alto" in v,
        },
    },

    # ─── Ground transport (Phase 2 Stage 1b) ──────────────────────────────
    # Three Lyft "Ride Report" PDFs and three Uber receipt PDFs. All
    # six are US-domestic. Expected values were eyeballed from the
    # original PDFs directly. Predicates (lambdas) are used where the
    # model has reasonable freedom in formatting (e.g. "101 California
    # Ave" vs "101 California Ave, Palo Alto"); literals are used
    # where the receipt prints an unambiguous value.
    {
        "image": "receipts/lyft1.pdf",
        "output": ".scratch/spike/lyft1.json",
        "extractor": "transport",
        "expect": {
            "common.date.value": "2026-04-12",
            "common.line_amount_usd.value": 6.75,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "ground_transportation_domestic",
            "common.country_of_activity.value": "United States",
            "ground_transport_details.service_provider.value": lambda v: v and "Lyft" in v,
            "ground_transport_details.origin.value": lambda v: v and "Oxford" in v,
            "ground_transport_details.destination.value": lambda v: v and "Bowdoin" in v,
            "extras.printed_currency.value": "USD",
        },
    },
    {
        "image": "receipts/lyft2.pdf",
        "output": ".scratch/spike/lyft2.json",
        "extractor": "transport",
        "expect": {
            "common.date.value": "2026-04-23",
            "common.line_amount_usd.value": 6.77,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "ground_transportation_domestic",
            "common.country_of_activity.value": "United States",
            "ground_transport_details.service_provider.value": lambda v: v and "Lyft" in v,
            "ground_transport_details.origin.value": lambda v: v and "Campus Dr" in v,
            "ground_transport_details.destination.value": lambda v: v and "California Ave" in v,
            "extras.printed_currency.value": "USD",
        },
    },
    {
        "image": "receipts/lyft3.pdf",
        "output": ".scratch/spike/lyft3.json",
        "extractor": "transport",
        "expect": {
            "common.date.value": "2026-03-29",
            "common.line_amount_usd.value": 60.48,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "ground_transportation_domestic",
            "common.country_of_activity.value": "United States",
            "ground_transport_details.service_provider.value": lambda v: v and "Lyft" in v,
            "ground_transport_details.origin.value": lambda v: v and "Airport" in v,
            "ground_transport_details.destination.value": lambda v: v and "Campus Dr" in v,
            "extras.printed_currency.value": "USD",
        },
    },
    {
        "image": "receipts/uber1.pdf",
        "output": ".scratch/spike/uber1.json",
        "extractor": "transport",
        "expect": {
            "common.date.value": "2026-04-20",
            "common.line_amount_usd.value": 8.95,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "ground_transportation_domestic",
            "common.country_of_activity.value": "United States",
            "ground_transport_details.service_provider.value": lambda v: v and "Uber" in v,
            "ground_transport_details.origin.value": lambda v: v and "Jane Stanford" in v,
            "ground_transport_details.destination.value": lambda v: v and "Campus Dr" in v,
            "extras.printed_currency.value": "USD",
        },
    },
    {
        "image": "receipts/uber2.pdf",
        "output": ".scratch/spike/uber2.json",
        "extractor": "transport",
        "expect": {
            "common.date.value": "2026-05-01",
            "common.line_amount_usd.value": 46.93,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "ground_transportation_domestic",
            "common.country_of_activity.value": "United States",
            "ground_transport_details.service_provider.value": lambda v: v and "Uber" in v,
            "ground_transport_details.origin.value": lambda v: v and "Campus Dr" in v,
            "ground_transport_details.destination.value": lambda v: v and "Tennessee" in v,
            "extras.printed_currency.value": "USD",
        },
    },
    {
        "image": "receipts/uber3.pdf",
        "output": ".scratch/spike/uber3.json",
        "extractor": "transport",
        "expect": {
            "common.date.value": "2026-03-29",
            "common.line_amount_usd.value": 46.24,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "ground_transportation_domestic",
            "common.country_of_activity.value": "United States",
            "ground_transport_details.service_provider.value": lambda v: v and "Uber" in v,
            "ground_transport_details.origin.value": lambda v: v and "Getty Center" in v,
            "ground_transport_details.destination.value": lambda v: v and "Broadway" in v,
            "extras.printed_currency.value": "USD",
        },
    },

    # ─── Lodging (Phase 3 Stage 1) ────────────────────────────────────────
    # English-only US hotel folios for v1. Multilingual (the German/French/
    # Japanese folios in the corpus) is a v2 follow-up per the FA's request
    # to keep the first cut simple. Predicates use loose `contains` matches
    # for hotel name + location since the model has formatting freedom;
    # literals for dates and totals (verified by direct PDF read).
    {
        "image": "receipts/Hyatt-Jan13-14.pdf",
        "output": ".scratch/spike/hyatt-jan13-14.json",
        "extractor": "lodging",
        "expect": {
            "common.line_amount_usd.value": 200.68,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "lodging_domestic",
            "common.country_of_activity.value": "United States",
            "lodging_details.hotel_name.value": lambda v: v and "Hyatt Place" in v,
            "lodging_details.location.value": lambda v: v and "Las Vegas" in v,
            "lodging_details.check_in_date.value": "2024-01-13",
            "lodging_details.check_out_date.value": "2024-01-14",
            "lodging_details.is_shared_lodging.value": False,
            "extras.printed_currency.value": "USD",
            # 1-night stay → exactly 1 nightly_rates entry. Reduction
            # averages this trivially to populate daily_rate.
            "extras.nightly_rates.value": lambda v: v and len(v) == 1
                and abs(v[0]["rate"] - 177.0) < 0.01,
        },
    },
    {
        "image": "receipts/Sheraton-Novi-14-21.pdf",
        "output": ".scratch/spike/sheraton-novi-14-21.json",
        "extractor": "lodging",
        "expect": {
            "common.line_amount_usd.value": 912.58,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "lodging_domestic",
            "common.country_of_activity.value": "United States",
            "lodging_details.hotel_name.value": lambda v: v and "Sheraton" in v,
            "lodging_details.location.value": lambda v: v and "Novi" in v,
            "lodging_details.check_in_date.value": "2024-01-14",
            "lodging_details.check_out_date.value": "2024-01-20",
            "lodging_details.is_shared_lodging.value": False,
            "extras.printed_currency.value": "USD",
            # 6-night flat-rate stay. Reduction's average should equal
            # the per-night rate.
            "extras.nightly_rates.value": lambda v: v and len(v) == 6
                and all(abs(n["rate"] - 134.0) < 0.01 for n in v),
        },
    },
    {
        "image": "receipts/Hilton-3185261353-SFO3-5-Jan.pdf",
        "output": ".scratch/spike/hilton-sfo-3-5-jan.json",
        "extractor": "lodging",
        "expect": {
            "common.expense_type.value": "lodging_domestic",
            "common.country_of_activity.value": "United States",
            "lodging_details.hotel_name.value": lambda v: v and "Hilton" in v,
            "lodging_details.is_shared_lodging.value": False,
            "extras.printed_currency.value": "USD",
            # Loose: not yet read in detail; verify only the kind/shape.
            "extras.nightly_rates.value": lambda v: v and len(v) >= 1,
        },
    },
    {
        "image": "receipts/Homewood-Suites-19-22-Sep.pdf",
        "output": ".scratch/spike/homewood-19-22-sep.json",
        "extractor": "lodging",
        "expect": {
            "common.expense_type.value": "lodging_domestic",
            "common.country_of_activity.value": "United States",
            "lodging_details.hotel_name.value": lambda v: v and "Homewood" in v,
            "lodging_details.is_shared_lodging.value": False,
            "extras.printed_currency.value": "USD",
            "extras.nightly_rates.value": lambda v: v and len(v) >= 1,
        },
    },
    {
        "image": "receipts/lodging_2026-01-04_hyatt-place-las-vegas.pdf",
        "output": ".scratch/spike/hyatt-vegas-jan-2026.json",
        "extractor": "lodging",
        "expect": {
            "common.expense_type.value": "lodging_domestic",
            "common.country_of_activity.value": "United States",
            "lodging_details.hotel_name.value": lambda v: v and "Hyatt" in v,
            "lodging_details.location.value": lambda v: v and "Las Vegas" in v,
            "extras.printed_currency.value": "USD",
            "extras.nightly_rates.value": lambda v: v and len(v) >= 1,
        },
    },
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--run",
        action="store_true",
        help="Re-invoke extract_meal.py before checking (3 Gemini calls; "
        "uses Application Default Credentials, run "
        "`gcloud auth application-default login` first).",
    )
    parser.add_argument(
        "--end-to-end",
        action="store_true",
        help="After per-receipt checks, run the Rust reduction binary and "
        "assert the aggregated ExpenseReport.",
    )
    return parser.parse_args()


def run_extractor(image: Path, output: Path, kind: str) -> None:
    extractor = EXTRACTORS.get(kind)
    if extractor is None:
        raise SystemExit(f"unknown extractor kind {kind!r} (known: {sorted(EXTRACTORS)})")
    subprocess.run(
        [
            str(PYTHON),
            str(extractor),
            "--image",
            str(image),
            "--output",
            str(output),
        ],
        check=True,
    )


def lookup(line: dict, dotted: str) -> Any:
    obj: Any = line
    for part in dotted.split("."):
        obj = obj[part]
    return obj


def check_one(name: str, line: dict, expectations: dict[str, Any]) -> list[str]:
    failures = []
    for path, expected in expectations.items():
        try:
            actual = lookup(line, path)
        except (KeyError, TypeError) as err:
            failures.append(f"  {path}: missing ({err})")
            continue
        if callable(expected):
            if not expected(actual):
                failures.append(f"  {path}: predicate failed (actual={actual!r})")
        elif actual != expected:
            failures.append(f"  {path}: expected {expected!r}, got {actual!r}")
    return failures


def main() -> int:
    args = parse_args()

    if args.run:
        for entry in RECEIPTS:
            image = REPO_ROOT / entry["image"]
            output = REPO_ROOT / entry["output"]
            kind = entry["extractor"]
            print(f"running {kind} extractor on {image.name} ...")
            run_extractor(image, output, kind)

    total_failures = 0
    for entry in RECEIPTS:
        output = REPO_ROOT / entry["output"]
        if not output.exists():
            print(f"FAIL  {output.name}: not found (run with --run first)")
            total_failures += 1
            continue

        data = json.loads(output.read_text())
        if not isinstance(data, list) or len(data) != 1:
            print(f"FAIL  {output.name}: top-level not an array of length 1 (got {type(data).__name__})")
            total_failures += 1
            continue
        line = data[0]
        # The expense_kind discriminator was dropped (Phase 1 Pair B): the
        # per-kind extractor router knows the kind from the FA's upload-form
        # choice. The presence of `meal_details` is the structural signal
        # that this is a meal line.
        if "meal_details" not in line:
            print(f"FAIL  {output.name}: meal_details missing (got top-level keys: {list(line.keys())})")
            total_failures += 1
            continue

        failures = check_one(output.name, line, entry["expect"])
        if failures:
            print(f"FAIL  {output.name}:")
            for line_msg in failures:
                print(line_msg)
            total_failures += len(failures)
        else:
            print(f"PASS  {output.name}")

    total_failures += run_roundtrip_check()

    if args.end_to_end:
        total_failures += run_end_to_end_check()

    print()
    if total_failures == 0:
        print(f"OK — all {len(RECEIPTS)} receipts pass.")
        return 0
    print(f"FAILED — {total_failures} assertion(s) across {len(RECEIPTS)} receipts.")
    return 1


def run_roundtrip_check() -> int:
    """Round-trip each .scratch/spike/*.json through the Rust ExtractedReceipt
    deserializer + serializer. Catches silent contract drift between Python
    and Rust. No Gemini calls."""
    print()
    print("round-trip: deserialize each spike output through Rust + diff ...")
    result = subprocess.run(
        ["cargo", "run", "--quiet", "--bin", "roundtrip_check"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    # roundtrip_check prints PASS/FAIL per file to stdout; relay it.
    if result.stdout.strip():
        for line in result.stdout.strip().splitlines():
            print(line)
    if result.returncode != 0:
        return max(result.stdout.count("FAIL"), 1)
    return 0


def run_end_to_end_check() -> int:
    """Run the Rust reduction binary and assert the aggregated ExpenseReport
    looks right. Returns the number of failed assertions (0 = pass)."""
    print()
    print("end-to-end: running cargo reduce_extractions ...")
    out_path = REPO_ROOT / ".scratch" / "reduced" / "report.json"
    result = subprocess.run(
        ["cargo", "run", "--quiet", "--bin", "reduce_extractions"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"FAIL  reduce_extractions exited {result.returncode}: {result.stderr.strip()}")
        return 1

    if not out_path.exists():
        print(f"FAIL  reduced report not found at {out_path}")
        return 1

    report = json.loads(out_path.read_text())
    expected_total = 163.54 + 123.19 + 387.12 + 79.59  # 753.44
    expectations = {
        "transaction_summary.total_usd": expected_total,
        "transaction_summary.transaction_date.value": "2026-03-05",
        "general_information.category.value": "expenses_domestic",
    }

    failures = 0
    for path, expected in expectations.items():
        actual = lookup(report, path)
        if isinstance(expected, float):
            ok = isinstance(actual, (int, float)) and abs(actual - expected) < 1e-9
        else:
            ok = actual == expected
        if not ok:
            print(f"FAIL  reduced.{path}: expected {expected!r}, got {actual!r}")
            failures += 1

    line_count = len(report.get("transaction_lines") or [])
    if line_count != len(RECEIPTS):
        print(f"FAIL  reduced.transaction_lines: expected {len(RECEIPTS)} lines, got {line_count}")
        failures += 1

    if failures == 0:
        print(f"PASS  reduced report — {line_count} lines, total ${report['transaction_summary']['total_usd']:.2f}")
    return failures


if __name__ == "__main__":
    raise SystemExit(main())
