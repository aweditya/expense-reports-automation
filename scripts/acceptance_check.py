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
EXTRACTOR = REPO_ROOT / "scripts" / "extract_meal.py"
PYTHON = REPO_ROOT / ".venv" / "bin" / "python"


# Each entry is (image_filename, output_json_filename, expectations).
# Expectations: dict where keys are dotted paths into the transaction line
# (e.g., "common.date.value"), values are either a literal (strict equality)
# or a callable taking the actual value and returning True/False.
RECEIPTS = [
    {
        "image": "receipts/mels1.jpeg",
        "output": ".scratch/spike/mels1.json",
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


def run_extractor(image: Path, output: Path) -> None:
    subprocess.run(
        [
            str(PYTHON),
            str(EXTRACTOR),
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
            print(f"running extractor on {image.name} ...")
            run_extractor(image, output)

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
