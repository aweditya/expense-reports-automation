#!/usr/bin/env python3
"""Generate the SDK response_schema for a list of meal transaction lines.

Reads `schema.yaml` and emits `generated/response_schema_meal.json`. Only the
meal expense kind is generated — other expense kinds get added to this script
when receipts of those kinds appear in the corpus.

The output is OpenAPI-3-style (which is what the Google Gen AI SDK accepts):
- `nullable: true` for fields whose value can be null
- enums via `"enum": [...]`
- no `oneOf` / `anyOf` / `$ref` (limited SDK support)

Hand-built rather than auto-derived from the YAML — this is a small, focused
slice and a generic converter would be more code than the slice itself.
"""

from __future__ import annotations

import json
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH = REPO_ROOT / "schema.yaml"
OUTPUT_PATH = REPO_ROOT / "generated" / "response_schema_meal.json"


def meta_block_schema() -> dict:
    """The _meta wrapper required on every leaf."""
    return {
        "type": "object",
        "properties": {
            "confidence": {"type": "string", "enum": ["high", "medium", "low"]},
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
            # source_documents is intentionally NOT in the per-receipt
            # schema — the FA gave us the file (we already know its name and
            # type). Reduction populates source_documents from the input
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


def extras_block_schema() -> dict:
    """Per-receipt extras — extracted from the receipt but NOT submitted to
    the FA portal. Reduction reads these to derive schema fields the model
    can't know in isolation (foreign vs domestic, original currency, FX).
    See Architecture B in docs/redesign-plan.md.
    """
    return {
        "type": "object",
        "properties": {
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
        },
        "required": ["merchant_address", "printed_currency"],
    }


def transaction_line_schema(expense_type_values: list[str]) -> dict:
    return {
        "type": "object",
        "properties": {
            "expense_kind": {"type": "string", "enum": ["meal"]},
            "common": common_block_schema(expense_type_values),
            "meal_details": meal_details_block_schema(),
            "extras": extras_block_schema(),
        },
        "required": ["expense_kind", "common", "meal_details", "extras"],
    }


def build_meal_response_schema(schema_yaml: dict) -> dict:
    common = schema_yaml["expense_report"]["transaction_lines"]["items"]["fields"]["common"]
    expense_type_values = list(common["expense_type"]["allowed_values"])
    return {
        "type": "array",
        "items": transaction_line_schema(expense_type_values),
    }


def main() -> int:
    schema_yaml = yaml.safe_load(SCHEMA_PATH.read_text())
    response_schema = build_meal_response_schema(schema_yaml)

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_PATH.write_text(json.dumps(response_schema, indent=2) + "\n")
    print(f"wrote {OUTPUT_PATH.relative_to(REPO_ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
