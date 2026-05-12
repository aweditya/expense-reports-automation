#!/usr/bin/env python3
"""Generate per-kind SDK response_schemas from schema.yaml.

Reads `schema.yaml` and emits `generated/response_schema_<kind>.json`
for each expense kind we extract today (meal, transport). Each per-kind
schema:
- restricts `expense_type` to only the variants relevant to that kind
  (so Gemini can't return `airfare_domestic` for a meal receipt);
- includes only that kind's detail block (e.g. `meal_details`) plus
  the shared `common` and `extras` blocks.

The output is OpenAPI-3-style (which is what the Google Gen AI SDK accepts):
- `nullable: true` for fields whose value can be null
- enums via `"enum": [...]`
- no `oneOf` / `anyOf` / `$ref` (limited SDK support)

Hand-built rather than auto-derived from the YAML — this is a small,
focused slice and a generic converter would be more code than the
slice itself. The KIND_EXPENSE_TYPES dict below declares which
schema.yaml enum variants belong to each kind. Adding a new kind =
add an entry to KIND_EXPENSE_TYPES + a detail-block function + an
entry in `KINDS` at the bottom.
"""

from __future__ import annotations

import json
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH = REPO_ROOT / "schema.yaml"
OUTPUT_DIR = REPO_ROOT / "generated"


# Which schema.yaml `expense_type` enum variants belong to each
# extractor kind. Each per-kind response_schema restricts the enum to
# only these values, so the SDK rejects an out-of-bucket guess (e.g.
# Gemini emitting `business_meal` for a Lyft receipt).
KIND_EXPENSE_TYPES: dict[str, list[str]] = {
    "meal": ["business_meal", "group_travel_meal"],
    "transport": ["ground_transportation_domestic", "ground_transportation_foreign"],
    "lodging": ["lodging_domestic", "lodging_foreign"],
}


def meta_block_schema() -> dict:
    """The _meta wrapper required on every leaf."""
    return {
        "type": "object",
        "properties": {
            "confidence": {"type": "string", "enum": ["high", "medium", "low"]},
            # One short sentence justifying the confidence choice. Optional
            # in the schema (so partial outputs still parse if the model
            # truncates) but the prompt asks for it on medium/low fields,
            # which is the only case the workbench renders it. High cards
            # often skip it — they're noise to justify when the value is
            # clearly printed.
            "confidence_reason": {"type": "string"},
            "evidence": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": [
                                "document_span",
                                "document",
                                "system_generated",
                                "user_input",
                            ],
                        },
                        "filename": {"type": "string", "nullable": True},
                        "page": {"type": "integer", "nullable": True},
                        "quote": {"type": "string", "nullable": True},
                        "origin": {"type": "string", "nullable": True},
                    },
                    "required": ["kind"],
                },
            },
            "needs_review": {"type": "boolean"},
            "flags": {"type": "array", "items": {"type": "string"}},
        },
        "required": ["confidence", "evidence", "needs_review", "flags"],
    }


def leaf(value_schema: dict) -> dict:
    """Wrap a value schema with the {value, _meta} convention."""
    return {
        "type": "object",
        "properties": {"value": value_schema, "_meta": meta_block_schema()},
        "required": ["value", "_meta"],
    }


def common_block_schema(expense_type_values: list[str]) -> dict:
    """The `common` block shared by every transaction line.

    `exchange_rate` is intentionally omitted: it is T2 (system-derived from FX
    lookup), not something the extractor can know from a single receipt.
    """
    return {
        "type": "object",
        "properties": {
            "date": leaf({"type": "string", "description": "ISO 8601 date (YYYY-MM-DD)"}),
            # Money is a number (f64). f64 has ~15-17 decimal digits of
            # precision — sufficient for travel-expense amounts.
            "line_amount_usd": leaf({"type": "number"}),
            "original_currency": leaf(
                {"type": "string", "nullable": True, "description": "ISO 4217 code or null if USD"}
            ),
            "original_amount": leaf({"type": "number", "nullable": True}),
            "expense_type": leaf({"type": "string", "enum": expense_type_values}),
            "remarks": leaf({"type": "string"}),
            "country_of_activity": leaf({"type": "string", "nullable": True}),
            "foreign_activity_type": leaf(
                {
                    "type": "string",
                    "nullable": True,
                    "enum": ["conference", "research_collaboration", "fieldwork", "other"],
                }
            ),
            # source_document is intentionally NOT in the per-receipt
            # schema — the FA gave us the file (we already know its name and
            # type). Reduction populates source_document from the input
            # context. See Architecture B in docs/redesign-plan.md.
        },
        "required": [
            "date",
            "line_amount_usd",
            "original_currency",
            "original_amount",
            "expense_type",
            "remarks",
            "country_of_activity",
            "foreign_activity_type",
        ],
    }


def meal_details_block_schema() -> dict:
    return {
        "type": "object",
        "properties": {
            "venue_name": leaf({"type": "string"}),
            # attendees and meal_purpose are intentionally NOT in the
            # per-receipt schema — both are T1 (FA fills later). See
            # Architecture B in docs/redesign-plan.md.
            "alcohol_amount": leaf({"type": "number", "nullable": True}),
            "tip_amount": leaf({"type": "number", "nullable": True}),
            "has_alcohol_on_receipt": leaf({"type": "boolean"}),
        },
        "required": [
            "venue_name",
            "alcohol_amount",
            "tip_amount",
            "has_alcohol_on_receipt",
        ],
    }


def ground_transport_details_block_schema() -> dict:
    return {
        "type": "object",
        "properties": {
            "origin": leaf({"type": "string"}),
            "destination": leaf({"type": "string"}),
            "service_provider": leaf({"type": "string"}),
            # missing_receipt is intentionally NOT in the per-receipt
            # schema — it's T1 (FA fills later if a receipt was lost).
        },
        "required": ["origin", "destination", "service_provider"],
    }


def lodging_details_block_schema() -> dict:
    """Per-receipt lodging detail block.

    Excludes:
      - number_of_nights: T2, computed by reduction from check_in/check_out.
      - daily_rate: T2, computed by reduction from extras.nightly_rates[]
        (mean of the nightly rates). The FA-facing single rate stays simple
        while the per-night breakdown lives in the per-document JSON for
        audit.
      - shared_with_transaction_number: T1, FA fills only when relevant.
      - personal_nights_excluded: T2, derived from conference dates.
    """
    return {
        "type": "object",
        "properties": {
            "hotel_name": leaf({"type": "string"}),
            "location": leaf(
                {
                    "type": "string",
                    "description": "City and country (e.g. 'San Francisco, USA').",
                }
            ),
            "check_in_date": leaf(
                {"type": "string", "description": "ISO 8601 date (YYYY-MM-DD)"}
            ),
            "check_out_date": leaf(
                {"type": "string", "description": "ISO 8601 date (YYYY-MM-DD)"}
            ),
            "booking_method": leaf(
                {
                    "type": "string",
                    "enum": [
                        "conference_hotel",
                        "stanford_travel_egencia",
                        "stanford_travel_key_travel",
                        "other",
                    ],
                }
            ),
            "is_shared_lodging": leaf({"type": "boolean"}),
        },
        "required": [
            "hotel_name",
            "location",
            "check_in_date",
            "check_out_date",
            "booking_method",
            "is_shared_lodging",
        ],
    }


def nightly_rates_schema() -> dict:
    """Per-night rate breakdown — only emitted by the lodging extractor.
    Reduction averages the rates to populate lodging_details.daily_rate.

    Bare array (no leaf wrapper). A leaf-wrapped array (`{value: array,
    _meta: {...}}`) is rejected by Vertex's Schema validator with a
    generic 400 — the validator doesn't accept leaves whose value is
    an array of objects. The signal we'd have carried on the wrapper
    (single confidence) shows up implicitly: reduction marks the
    derived `daily_rate.meta.confidence` as `high` when the breakdown
    is present, `low` when absent.
    """
    return {
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "date": {
                    "type": "string",
                    "description": "ISO 8601 date (YYYY-MM-DD)",
                },
                "rate": {
                    "type": "number",
                    "description": "Room rate that night, in the printed currency.",
                },
                "taxes_and_fees": {
                    "type": "number",
                    "description": "Sum of all taxes/fees that night (VAT, occupancy tax, city tax). Zero if not broken out.",
                },
            },
            "required": ["date", "rate", "taxes_and_fees"],
        },
    }


def extras_block_schema(include_nightly_rates: bool = False) -> dict:
    """Per-receipt extras — extracted from the receipt but NOT submitted to
    the FA portal. Reduction reads these to derive schema fields the model
    can't know in isolation (foreign vs domestic, original currency, FX,
    daily_rate average for lodging). See Architecture B in docs/redesign-plan.md.

    `include_nightly_rates`: only the lodging kind needs the per-night
    breakdown — meal and transport pass through with the merchant_address
    + printed_currency pair only.
    """
    properties = {
        "merchant_address": leaf(
            {
                "type": "string",
                "nullable": True,
                "description": "Full printed merchant address (street + city + region + country if visible).",
            }
        ),
        "printed_currency": leaf(
            {
                "type": "string",
                "nullable": True,
                "description": "ISO 4217 code if printed, else best-effort (e.g. 'USD' from a $ sign).",
            }
        ),
    }
    required = ["merchant_address", "printed_currency"]
    if include_nightly_rates:
        properties["nightly_rates"] = nightly_rates_schema()
        required.append("nightly_rates")
    return {
        "type": "object",
        "properties": properties,
        "required": required,
    }


def transaction_line_schema(
    expense_type_values: list[str],
    detail_block_name: str,
    detail_block_schema: dict,
    include_nightly_rates: bool = False,
) -> dict:
    """One transaction line as Gemini emits it for a single receipt.

    The expense_kind discriminator was dropped (Phase 1 Pair B): the
    per-kind extractor router (`scripts/extract_<kind>.py`) knows the
    kind from the FA's upload-form choice — it doesn't need to be
    repeated inside the per-document JSON. The per-kind detail block
    (passed in here) is the structural discriminator instead.

    `include_nightly_rates` adds extras.nightly_rates (lodging only).
    """
    return {
        "type": "object",
        "properties": {
            "common": common_block_schema(expense_type_values),
            detail_block_name: detail_block_schema,
            "extras": extras_block_schema(include_nightly_rates=include_nightly_rates),
        },
        "required": ["common", detail_block_name, "extras"],
    }


def build_response_schema(
    expense_type_values: list[str],
    detail_block_name: str,
    detail_block_schema: dict,
    include_nightly_rates: bool = False,
) -> dict:
    return {
        "type": "array",
        "items": transaction_line_schema(
            expense_type_values,
            detail_block_name,
            detail_block_schema,
            include_nightly_rates=include_nightly_rates,
        ),
    }


# Per-kind output specs. Adding a new kind: extend KIND_EXPENSE_TYPES,
# add a `<kind>_details_block_schema()` function above, and append an
# entry here. The 4th tuple item is `include_nightly_rates` — only
# lodging needs it (per-night breakdown for daily_rate averaging).
KINDS: list[tuple[str, str, callable, bool]] = [
    ("meal", "meal_details", meal_details_block_schema, False),
    ("transport", "ground_transport_details", ground_transport_details_block_schema, False),
    ("lodging", "lodging_details", lodging_details_block_schema, True),
]


def validate_kind_expense_types(schema_yaml: dict) -> None:
    """Sanity check: every kind's expense_type values must exist in the
    schema.yaml master enum. Catches typos / drift between the codegen's
    KIND_EXPENSE_TYPES dict and the schema's allowed_values list."""
    common = schema_yaml["expense_report"]["transaction_lines"]["items"]["fields"]["common"]
    master = set(common["expense_type"]["allowed_values"])
    for kind, values in KIND_EXPENSE_TYPES.items():
        unknown = set(values) - master
        if unknown:
            raise SystemExit(
                f"KIND_EXPENSE_TYPES[{kind!r}] references unknown enum values "
                f"not in schema.yaml: {sorted(unknown)}"
            )


def main() -> int:
    schema_yaml = yaml.safe_load(SCHEMA_PATH.read_text())
    validate_kind_expense_types(schema_yaml)

    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    for kind, detail_block_name, detail_block_factory, include_nightly_rates in KINDS:
        response_schema = build_response_schema(
            KIND_EXPENSE_TYPES[kind],
            detail_block_name,
            detail_block_factory(),
            include_nightly_rates=include_nightly_rates,
        )
        out_path = OUTPUT_DIR / f"response_schema_{kind}.json"
        out_path.write_text(json.dumps(response_schema, indent=2) + "\n")
        print(f"wrote {out_path.relative_to(REPO_ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
