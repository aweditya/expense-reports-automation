#!/usr/bin/env python3
"""Acceptance check for the spike extractor on the three real receipts.

By default reads the JSON files already in `.scratch/spike/`. With `--run`,
re-invokes the extractor against `receipts/{mels1.jpeg,mels2.jpeg,tamarine.png}`
first. A real run costs three Gemini calls (~2 min) so this is a manual
sanity script, not a CI test.

Per-receipt assertions are hand-coded based on observed M5.3 output. They
cover the values where being wrong matters (date, total, venue, expense
kind, alcohol presence) and tolerate model noise on volatile fields
(`expense_type` allowed to be either business_meal or
business_meal_with_alcohol when alcohol is on the receipt).
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
EXTRACTOR = REPO_ROOT / "scripts" / "spike_extract.py"
PYTHON = REPO_ROOT / ".venv" / "bin" / "python"
DEFAULT_KEY = REPO_ROOT / "soe-agile-agents-7581b31cd4d2.json"


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
            "common.line_amount_usd.value": "163.54",
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": lambda v: v
            in ("business_meal", "business_meal_with_alcohol"),
            "meal_details.venue_name.value": lambda v: v
            and "MJ Sushi" in v,
            "meal_details.has_alcohol_on_receipt.value": True,
        },
    },
    {
        "image": "receipts/mels2.jpeg",
        "output": ".scratch/spike/mels2.json",
        "expect": {
            "common.date.value": "2026-04-04",
            "common.line_amount_usd.value": "123.19",
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal",
            "meal_details.venue_name.value": lambda v: v
            and "Original Mels" in v,
            "meal_details.has_alcohol_on_receipt.value": False,
        },
    },
    {
        "image": "receipts/tamarine.png",
        "output": ".scratch/spike/tamarine.json",
        "expect": {
            "common.date.value": "2026-03-05",
            "common.line_amount_usd.value": "387.12",
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal_with_alcohol",
            "meal_details.venue_name.value": lambda v: v
            and "Tamarine" in v,
            "meal_details.has_alcohol_on_receipt.value": True,
        },
    },
    {
        "image": "receipts/mjsushi.jpeg",
        "output": ".scratch/spike/mjsushi.json",
        "expect": {
            "common.date.value": "2026-05-02",
            "common.line_amount_usd.value": "79.59",
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": lambda v: v
            in ("business_meal", "business_meal_with_alcohol"),
            "meal_details.venue_name.value": lambda v: v and "MJ Sushi" in v,
            "meal_details.has_alcohol_on_receipt.value": True,
        },
    },
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--run",
        action="store_true",
        help="Re-invoke spike_extract.py before checking (3 Gemini calls).",
    )
    parser.add_argument(
        "--service-account-key",
        type=Path,
        default=DEFAULT_KEY,
        help=f"Service account key for --run (default: {DEFAULT_KEY.name}).",
    )
    return parser.parse_args()


def run_extractor(image: Path, output: Path, key: Path) -> None:
    subprocess.run(
        [
            str(PYTHON),
            str(EXTRACTOR),
            "--image",
            str(image),
            "--output",
            str(output),
            "--service-account-key",
            str(key),
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
        if not args.service_account_key.exists():
            print(f"missing service account key: {args.service_account_key}", file=sys.stderr)
            return 2
        for entry in RECEIPTS:
            image = REPO_ROOT / entry["image"]
            output = REPO_ROOT / entry["output"]
            print(f"running extractor on {image.name} ...")
            run_extractor(image, output, args.service_account_key)

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
        if line.get("expense_kind") != "meal":
            print(f"FAIL  {output.name}: expense_kind != 'meal' (got {line.get('expense_kind')!r})")
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

    print()
    if total_failures == 0:
        print(f"OK — all {len(RECEIPTS)} receipts pass.")
        return 0
    print(f"FAILED — {total_failures} assertion(s) across {len(RECEIPTS)} receipts.")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
