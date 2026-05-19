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
    "airfare": REPO_ROOT / "scripts" / "extract_airfare.py",
}

# Detail block expected on a per-doc JSON for each extractor kind. The
# guard below uses this to fail fast if the extractor produced the
# wrong shape (e.g. a meal extractor emitting `ground_transport_details`).
DETAIL_BLOCK_BY_KIND = {
    "meal": "meal_details",
    "transport": "ground_transport_details",
    "lodging": "lodging_details",
    "airfare": "airfare_details",
}


# Each entry: image, output, extractor (which kind), expectations.
# Expectations: dict where keys are dotted paths into the transaction line
# (e.g., "common.date.value"), values are either a literal (strict equality)
# or a callable taking the actual value and returning True/False.
RECEIPTS = [
    # ─── Meals ────────────────────────────────────────────────────────────
    {
        "image": "receipts/meal_2026-04-04_original-mels-san-leandro.jpeg",
        "output": ".scratch/spike/meal-original-mels-san-leandro.json",
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
        "image": "receipts/meal_2026-04-19_mj-sushi-palo-alto.jpeg",
        "output": ".scratch/spike/meal-mj-sushi-2026-04-19.json",
        "extractor": "meal",
        "expect": {
            "common.date.value": "2026-04-19",
            "common.line_amount_usd.value": 163.54,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal",
            "meal_details.venue_name.value": lambda v: v and "MJ Sushi" in v,
            "meal_details.has_alcohol_on_receipt.value": True,
            "extras.printed_currency.value": "USD",
        },
    },
    {
        "image": "receipts/meal_2026-05-02_mj-sushi-palo-alto.jpeg",
        "output": ".scratch/spike/meal-mj-sushi-2026-05-02.json",
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
    {
        "image": "receipts/meal_2026-03-05_tamarine-palo-alto.jpg",
        "output": ".scratch/spike/meal-tamarine-palo-alto.json",
        "extractor": "meal",
        "expect": {
            "common.date.value": "2026-03-05",
            "common.line_amount_usd.value": 387.12,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal",
            "meal_details.venue_name.value": lambda v: v and "Tamarine" in v,
            "meal_details.has_alcohol_on_receipt.value": True,
            "extras.printed_currency.value": "USD",
            "extras.merchant_address.value": lambda v: v and "Palo Alto" in v,
        },
    },
    {
        "image": "receipts/meal_2026-05-10_palo-alto-creamery.jpg",
        "output": ".scratch/spike/meal-palo-alto-creamery.json",
        "extractor": "meal",
        "expect": {
            "common.date.value": "2026-05-10",
            "common.line_amount_usd.value": 163.50,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "business_meal",
            "meal_details.venue_name.value": lambda v: v
            and "Palo Alto Creamery" in v,
            "meal_details.has_alcohol_on_receipt.value": False,
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
        "image": "receipts/transport_2026-04-12_lyft-oxford-to-bowdoin.pdf",
        "output": ".scratch/spike/transport-lyft-oxford-to-bowdoin.json",
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
        "image": "receipts/transport_2026-04-23_lyft-campus-to-california.pdf",
        "output": ".scratch/spike/transport-lyft-campus-to-california.json",
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
        "image": "receipts/transport_2026-03-29_lyft-sfo-to-stanford.pdf",
        "output": ".scratch/spike/transport-lyft-sfo-to-stanford.json",
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
        "image": "receipts/transport_2026-04-20_uber-jane-stanford-to-campus.pdf",
        "output": ".scratch/spike/transport-uber-jane-stanford-to-campus.json",
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
        "image": "receipts/transport_2026-05-01_uber-stanford-to-sf-tennessee.pdf",
        "output": ".scratch/spike/transport-uber-stanford-to-sf-tennessee.json",
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
        "image": "receipts/transport_2026-03-29_uber-getty-to-broadway.pdf",
        "output": ".scratch/spike/transport-uber-getty-to-broadway.json",
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
        "image": "receipts/lodging_2024-01-13_hyatt-place-las-vegas-jan-13-14.pdf",
        "output": ".scratch/spike/lodging-hyatt-las-vegas-jan-13-14.json",
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
            "extras.nightly_rates": lambda v: v and len(v) == 1
                and abs(v[0]["rate"] - 177.0) < 0.01,
        },
    },
    {
        "image": "receipts/lodging_2024-01-14_sheraton-novi-folio.pdf",
        "output": ".scratch/spike/lodging-sheraton-novi-folio.json",
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
            "extras.nightly_rates": lambda v: v and len(v) == 6
                and all(abs(n["rate"] - 134.0) < 0.01 for n in v),
        },
    },
    {
        "image": "receipts/lodging_2025-01-03_hilton-garden-inn-palo-alto-confirmation.pdf",
        "output": ".scratch/spike/lodging-hilton-garden-inn-palo-alto-confirmation.json",
        "extractor": "lodging",
        "expect": {
            "common.expense_type.value": "lodging_domestic",
            "common.country_of_activity.value": "United States",
            "lodging_details.hotel_name.value": lambda v: v and "Hilton" in v,
            "lodging_details.is_shared_lodging.value": False,
            "extras.printed_currency.value": "USD",
            # Loose: not yet read in detail; verify only the kind/shape.
            "extras.nightly_rates": lambda v: v and len(v) >= 1,
        },
    },
    {
        "image": "receipts/lodging_2025-09-19_homewood-suites-palo-alto-confirmation.pdf",
        "output": ".scratch/spike/lodging-homewood-suites-palo-alto-confirmation.json",
        "extractor": "lodging",
        "expect": {
            "common.expense_type.value": "lodging_domestic",
            "common.country_of_activity.value": "United States",
            "lodging_details.hotel_name.value": lambda v: v and "Homewood" in v,
            "lodging_details.is_shared_lodging.value": False,
            "extras.printed_currency.value": "USD",
            "extras.nightly_rates": lambda v: v and len(v) >= 1,
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
            "extras.nightly_rates": lambda v: v and len(v) >= 1,
        },
    },

    # ─── Airfare (Phase 4 Stage 2) ────────────────────────────────────────
    # Four real-corpus tickets: Egencia/United (round-trip, Stanford
    # workflow), Air India (one-way international, INR currency triggers
    # the FX mock in reduction), Gmail-saved United e-ticket (round-trip
    # USD), Southwest (one-way USD, fare-family naming). Predicates are
    # loose for fields the model has formatting freedom on (airline
    # name, traveler name); literal where the ticket prints unambiguous
    # values (route IATAs, totals, dates). The 3-call multi-call
    # extractor merges results into a single airfare_details block; the
    # round_trip predicate lives in the flight-call subset, ticket_amount
    # in the booking-call subset, segments[] in the extras-call subset.
    {
        "image": "receipts/airfare_2026-03-21_egencia-united-sfo-pit-roundtrip.pdf",
        "output": ".scratch/spike/airfare-egencia-united-sfo-pit.json",
        "extractor": "airfare",
        "expect": {
            "common.line_amount_usd.value": 843.60,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "airfare_domestic",
            "common.country_of_activity.value": "United States",
            "airfare_details.airline.value": lambda v: v and "United" in v,
            "airfare_details.departure_airport.value": "SFO",
            "airfare_details.destination_airport.value": "PIT",
            "airfare_details.class_of_ticket.value": "coach",
            "airfare_details.round_trip.value": True,
            "airfare_details.booking_method.value": "stanford_travel_egencia",
            "airfare_details.ticket_amount.value": 843.60,
            "extras.printed_currency.value": "USD",
            # Round-trip = at least 2 segments (could be 4 with connections;
            # this particular Egencia is non-stop SFO↔PIT both ways).
            "extras.segments": lambda v: v and len(v) >= 2,
        },
    },
    {
        "image": "receipts/airfare_2024-09-02_air-india-bom-sfo-oneway.pdf",
        "output": ".scratch/spike/airfare-air-india-bom-sfo.json",
        "extractor": "airfare",
        "expect": {
            # USD null because reduction does the FX (mock rate ~$0.012/INR).
            # Predicate lives in the FX-mock test in src/reduce.rs, not here.
            "common.line_amount_usd.value": None,
            "common.original_currency.value": "INR",
            "common.original_amount.value": 80896,
            "common.expense_type.value": "airfare_foreign",
            "airfare_details.airline.value": lambda v: v and "Air India" in v,
            "airfare_details.departure_airport.value": "BOM",
            "airfare_details.destination_airport.value": "SFO",
            "airfare_details.round_trip.value": False,
            "airfare_details.ticket_amount.value": 80896,
            "extras.printed_currency.value": "INR",
            # One-way = at least 1 segment (BOM→SFO direct, sometimes
            # split via DEL/HKG depending on routing).
            "extras.segments": lambda v: v and len(v) >= 1,
        },
    },
    {
        "image": "receipts/airfare_2025-11-23_united-sfo-ord-roundtrip.pdf",
        "output": ".scratch/spike/airfare-united-sfo-ord.json",
        "extractor": "airfare",
        "expect": {
            "common.line_amount_usd.value": 519.97,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "airfare_domestic",
            "common.country_of_activity.value": "United States",
            "airfare_details.airline.value": lambda v: v and "United" in v,
            "airfare_details.departure_airport.value": "SFO",
            "airfare_details.destination_airport.value": "ORD",
            "airfare_details.class_of_ticket.value": "coach",
            "airfare_details.round_trip.value": True,
            "airfare_details.ticket_amount.value": 519.97,
            "extras.printed_currency.value": "USD",
            "extras.segments": lambda v: v and len(v) >= 2,
        },
    },
    {
        "image": "receipts/airfare_2026-03-23_southwest-sfo-phx-oneway.pdf",
        "output": ".scratch/spike/airfare-southwest-sfo-phx.json",
        "extractor": "airfare",
        "expect": {
            "common.line_amount_usd.value": 102.40,
            "common.original_currency.value": None,
            "common.original_amount.value": None,
            "common.expense_type.value": "airfare_domestic",
            "common.country_of_activity.value": "United States",
            "airfare_details.airline.value": lambda v: v and "Southwest" in v,
            "airfare_details.departure_airport.value": "SFO",
            "airfare_details.destination_airport.value": "PHX",
            # Southwest "Basic" maps to coach.
            "airfare_details.class_of_ticket.value": "coach",
            "airfare_details.round_trip.value": False,
            "airfare_details.ticket_amount.value": 102.40,
            "extras.printed_currency.value": "USD",
            "extras.segments": lambda v: v and len(v) >= 1,
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
            if not image.exists():
                # Stale corpus reference — some entries point at filenames
                # the receipts/ directory no longer has (renamed during a
                # cleanup pass). Skip rather than aborting the whole run;
                # the missing image will surface as a "not found" failure
                # in the assertions phase below if no cached spike file
                # exists either.
                print(f"skip  {kind} extractor: image not found ({image.name})")
                continue
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
        # The expense_kind discriminator was dropped (Phase 1 Pair B):
        # the per-kind extractor router knows the kind from the FA's
        # upload-form choice. The presence of the matching detail block
        # is the structural signal that the extractor emitted the right
        # shape — meal_details for meal, lodging_details for lodging, etc.
        expected_detail_block = DETAIL_BLOCK_BY_KIND[entry["extractor"]]
        if expected_detail_block not in line:
            print(
                f"FAIL  {output.name}: {expected_detail_block} missing "
                f"(got top-level keys: {list(line.keys())})"
            )
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


if __name__ == "__main__":
    raise SystemExit(main())
