#!/usr/bin/env python3
"""Generate per-kind SDK response_schemas from schema.yaml.

Reads `schema.yaml` and emits one or more
`generated/response_schema_<name>.json` files. Multi-call kinds (the
ones whose typed detail block pushes past Vertex's schema property-
count ceiling) split into `<kind>_main` + `<kind>_extras` schema
files that get merged in `scripts/extract_<kind>.py`. Today, 
split applies to meal, transport, lodging (which also includes the
per-night-rate breakdown in its extras) and airfare (which uses a
3-call split: main + aux + extras). Single-call kinds — miscellaneous,
membership, conference_registration — emit one file each.

The split-call pattern is forced by Vertex's response-schema
property-count ceiling. The orchestration of parallel calls lives
in each `scripts/extract_<kind>.py`; this file produces only the
schemas.

Each schema:
- restricts `expense_type` (when `common` is included) to only the
  variants relevant to that kind, so Gemini can't return
  `airfare_domestic` for a meal receipt;
- includes only the detail block (and/or extras) the corresponding
  Gemini call is supposed to extract.

The output is OpenAPI-3-style (which is what the Google Gen AI SDK
accepts):
- `nullable: true` for fields whose value can be null
- enums via `"enum": [...]`
- no `oneOf` / `anyOf` / `$ref` (proved out via
  `scripts/probe_response_schemas.py`; both `response_schema` and
  `response_json_schema` reject `$ref` at different layers)

Hand-built rather than auto-derived from the YAML — this is a small,
focused slice and a generic converter would be more code than the
slice itself. Adding a new kind: add to `KIND_EXPENSE_TYPES`, add a
detail-block factory, append an entry (or entries) to
`SCHEMAS_TO_GENERATE` at the bottom.
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
    "airfare": ["airfare_domestic", "airfare_foreign"],
    # Conference registration has no domestic/foreign distinction in the
    # master enum — same enum value for both. The trip's foreignness is
    # signaled by the receipt's currency / the trip's airfare line.
    "conference_registration": ["conference_registration"],
    # Catch-all for things that don't fit a dedicated kind: posters,
    # printing services, software subscriptions for the trip, etc.
    # Schema maps `other_business_expense` to "Miscellaneous" on the
    # domestic CSV and "Miscellaneous - Foreign" on the foreign CSV.
    "miscellaneous": ["other_business_expense"],
    # Membership dues for a professional society / conference org. Maps
    # to "Membership Dues" on the domestic CSV and "Membership Dues -
    # Foreign" on the foreign CSV. No kind-specific detail block — the
    # receipt typically only carries org name + amount + date.
    "membership": ["membership_dues"],
    # Personal mileage reimbursement at IRS Standard Mileage Rate
    # (business use). The FA attaches a Google Maps screenshot / driving
    # log showing the route + distance; reduction multiplies by the
    # current IRS rate (generated/irs_mileage_rates.json) to get the
    # line amount.
    "mileage": ["personal_mileage"],
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
                        # When the extractor prompt includes a
                        # numbered Document AI token list, the model
                        # returns the integer IDs of the tokens it
                        # grounded against. evidence_bbox.populate_bboxes
                        # resolves IDs to bboxes. Omitting falls back
                        # to text-matching. Optional everywhere.
                        "token_ids": {
                            "type": "array",
                            "items": {"type": "integer"},
                            "nullable": True,
                        },
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
            # precision — sufficient for travel-expense amounts. `nullable`
            # so airfare's PROMPT_MAIN can leave this null on foreign
            # tickets (origin: needs_fx_conversion) for reduction's
            # mock_usd_rate to fill — without it, the model is forced to
            # emit a sentinel like 0 which apply_mock_fx then mistakes
            # for "already set." 
            "line_amount_usd": leaf({"type": "number", "nullable": True}),
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
            # context. See the multi-call architecture in docs/internals.md §5.
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
    # pre_tax_amount + tax_amount added back after the tip-cap
    # revert. The block fits now because the meal schema is split into
    # meal_main + meal_extras (extras moved to its own call), freeing
    # property budget under Vertex's ceiling.
    return {
        "type": "object",
        "properties": {
            "venue_name": leaf({"type": "string"}),
            # attendees and meal_purpose are intentionally NOT in the
            # per-receipt schema — both are T1 (FA fills later). See
            # the multi-call architecture in docs/internals.md §5.
            "alcohol_amount": leaf({"type": "number", "nullable": True}),
            "tip_amount": leaf({"type": "number", "nullable": True}),
            "has_alcohol_on_receipt": leaf({"type": "boolean"}),
            "pre_tax_amount": leaf({"type": "number", "nullable": True}),
            "tax_amount": leaf({"type": "number", "nullable": True}),
        },
        "required": [
            "venue_name",
            "alcohol_amount",
            "tip_amount",
            "has_alcohol_on_receipt",
            "pre_tax_amount",
            "tax_amount",
        ],
    }


def mileage_details_block_schema() -> dict:
    # Personal mileage. Small detail block, single-call extractor.
    # vehicle_class is T1 (FA picks, defaults to personal_car), so it's
    # NOT in the response schema. trip_date is T3 here because typical
    # Google Maps screenshots / driving logs have the date visible.
    return {
        "type": "object",
        "properties": {
            "distance_miles": leaf({"type": "number"}),
            "origin": leaf({"type": "string"}),
            "destination": leaf({"type": "string"}),
            "trip_date": leaf(
                {"type": "string", "description": "ISO 8601 date (YYYY-MM-DD)"}
            ),
        },
        "required": ["distance_miles", "origin", "destination", "trip_date"],
    }


def ground_transport_details_block_schema() -> dict:
    # tip_amount + pre_tax_amount + tax_amount added back after the
    # tip-cap revert. The block fits because the transport schema
    # is split into transport_main + transport_extras.
    return {
        "type": "object",
        "properties": {
            "origin": leaf({"type": "string"}),
            "destination": leaf({"type": "string"}),
            "service_provider": leaf({"type": "string"}),
            # missing_receipt is intentionally NOT in the per-receipt
            # schema — it's T1 (FA fills later if a receipt was lost).
            "tip_amount": leaf({"type": "number", "nullable": True}),
            "pre_tax_amount": leaf({"type": "number", "nullable": True}),
            "tax_amount": leaf({"type": "number", "nullable": True}),
        },
        "required": [
            "origin",
            "destination",
            "service_provider",
            "tip_amount",
            "pre_tax_amount",
            "tax_amount",
        ],
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


def _airfare_detail_field_schemas() -> dict:
    """Per-leaf schemas for the 9 airfare_details fields. Defined once;
    `airfare_details_block_schema()` selects subsets by name for the
    3-call split. Keep in lock-step with schema.yaml's airfare_details."""
    return {
        "travelers_name": leaf({"type": "string"}),
        "ticket_number": leaf({"type": "string"}),
        # ticket_amount kept alongside common.line_amount_usd: it's the
        # printed amount on the ticket in whatever currency the ticket
        # prints. For domestic USD it equals line_amount_usd; for foreign
        # it equals common.original_amount and reduction uses it as the
        # source for FX into line_amount_usd.
        "ticket_amount": leaf({"type": "number"}),
        "booking_method": leaf(
            {
                "type": "string",
                "enum": [
                    "stanford_travel_egencia",
                    "stanford_travel_key_travel",
                    "stanford_travel_connect_ua",
                    "stanford_travel_connect_dl",
                    "stanford_travel_connect_aa",
                    "stanford_travel_connect_as",
                    "stanford_travel_connect_ha",
                    "other",
                ],
            }
        ),
        "airline": leaf({"type": "string"}),
        "class_of_ticket": leaf(
            {
                "type": "string",
                "enum": ["coach", "premium_economy", "business", "first"],
            }
        ),
        "departure_airport": leaf(
            {"type": "string", "description": "IATA code"}
        ),
        "destination_airport": leaf(
            {"type": "string", "description": "IATA code"}
        ),
        "round_trip": leaf({"type": "boolean"}),
    }


def airfare_details_block_schema(fields: list[str] | None = None) -> dict:
    """Per-receipt airfare detail block.

    Excludes:
      - price_comparison: T2/system-derived; out of scope for v1 (the
        FA either provides one or reduction looks one up). Removed
        from schema.yaml as part of the airfare extractor work.

    9 leaves total — over Vertex's schema property-count ceiling for a
    single call. The airfare extractor uses a 3-call split (lodging
    used 2; airfare needs 3 because its detail block is bigger):

      - `airfare_main`: common + 5 flight-centric fields
      - `airfare_aux`: 4 booking/payment-centric fields only
      - `airfare_extras`: extras + segments[] bare array

    Both `airfare_main` and `airfare_aux` emit under the same
    `airfare_details` key; the extractor 1-deep-merges them on the
    Python side before writing the per-doc JSON. Reduction sees the
    union as a single complete `airfare_details` object — same shape
    a hypothetical single-call would have produced.

    Pass `fields=None` for the full block (used by tests / single-call
    fallback). Pass a subset list for the per-call split.

    Probe locally with `scripts/probe_response_schemas.py` after any
    leaf-count change.
    """
    all_fields = _airfare_detail_field_schemas()
    if fields is None:
        fields = list(all_fields.keys())
    return {
        "type": "object",
        "properties": {k: all_fields[k] for k in fields},
        "required": list(fields),
    }


# Field-name groupings for the 3-call split. Defined as constants so
# `extract_airfare.py` can reuse them when building per-call prompts —
# keeps the prompt-vs-schema field lists from drifting.
AIRFARE_FLIGHT_FIELDS = [
    "airline",
    "departure_airport",
    "destination_airport",
    "class_of_ticket",
    "round_trip",
]
AIRFARE_BOOKING_FIELDS = [
    "travelers_name",
    "ticket_number",
    "ticket_amount",
    "booking_method",
]


def conference_registration_details_block_schema() -> dict:
    """Per-receipt conference-registration detail block.

    5 T3 leaves (only the model-extracted ones; conference_start_date /
    conference_end_date are T2-derived in reduction from supporting_doc
    aggregation, and meals_included is T1 — all absent from this
    response_schema). At the proven 5-leaf single-call ceiling.

    See docs/phase-5-design.md.
    """
    return {
        "type": "object",
        "properties": {
            "conference_name": leaf({"type": "string"}),
            "order_number": leaf({"type": "string"}),
            "ticket_type": leaf({"type": "string"}),
            "attendee_name": leaf({"type": "string"}),
            "registration_system": leaf(
                {
                    "type": "string",
                    "enum": [
                        "whova",
                        "cvent",
                        "acm_regonline",
                        "eventbrite",
                        "usenix",
                        "other",
                    ],
                }
            ),
        },
        "required": [
            "conference_name",
            "order_number",
            "ticket_type",
            "attendee_name",
            "registration_system",
        ],
    }


def supporting_conference_doc_response_schema() -> dict:
    """Response schema for supporting conference docs (program, schedule,
    papers, etc.).

    NEW per-doc shape with NO `common` block — supporting docs are not
    expenses. Just structured context. Two _meta-wrapped leaves
    (conference_name_as_printed, doc_kind) plus four bare arrays for
    collections (scheduled_dates, venues_mentioned, papers_listed,
    workshops_listed). Same bare-array rationale as nightly_rates /
    segments — no per-entry _meta to keep the output budget compact.

    Each field is independently optional in practice — a paper-only PDF
    fills papers_listed, leaves the others empty. Reduction collects
    whatever's populated across all supporting docs in the upload.

    See docs/phase-5-design.md.
    """
    return {
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "conference_name_as_printed": leaf({"type": "string"}),
                "doc_kind": leaf(
                    {
                        "type": "string",
                        "enum": ["program", "schedule", "papers", "other"],
                    }
                ),
                "scheduled_dates": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "description": "ISO 8601 date (YYYY-MM-DD)",
                    },
                },
                "venues_mentioned": {
                    "type": "array",
                    "items": {"type": "string"},
                },
                "papers_listed": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "authors_string": {
                                "type": "string",
                                "description": "Raw author list as printed (synthesis canonicalizes per-author later).",
                            },
                        },
                        "required": ["title", "authors_string"],
                    },
                },
                "workshops_listed": {
                    "type": "array",
                    "items": {"type": "string"},
                },
            },
            "required": [
                "conference_name_as_printed",
                "doc_kind",
                "scheduled_dates",
                "venues_mentioned",
                "papers_listed",
                "workshops_listed",
            ],
        },
    }


def synthesis_conference_bundle_response_schema() -> dict:
    """Response schema for the conference-bundle synthesis call (T4).

    The narrowest schema in the codebase. Four _meta-wrapped fuzzy
    leaves only — anything that could be derived from per-doc JSONs by
    a rule (date range, venue list, total cost) is structurally absent
    so the LLM is incapable of emitting it. Plus two provenance items
    (source_filenames, synthesis_confidence).

    See docs/phase-5-design.md.
    """
    return {
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "canonical_event_name": leaf({"type": "string"}),
                "participant_role": leaf(
                    {
                        "type": "string",
                        "enum": ["attendee", "presenter", "organizer", "other"],
                    }
                ),
                "business_purpose_what": leaf(
                    {
                        "type": "string",
                        "description": "≤120 chars; one short phrase about what the trip is for.",
                    }
                ),
                "business_purpose_why": leaf(
                    {
                        "type": "string",
                        "description": "≤200 chars; one short sentence about the FA-relevant reason.",
                    }
                ),
                "source_filenames": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Per-doc JSON filenames the synthesis read; for FA audit / workbench citation.",
                },
                "synthesis_confidence": leaf(
                    {
                        "type": "string",
                        "enum": ["high", "medium", "low"],
                        "description": "Synthesis self-rating; drives needs_review on lifted general_information fields.",
                    }
                ),
            },
            "required": [
                "canonical_event_name",
                "participant_role",
                "business_purpose_what",
                "business_purpose_why",
                "source_filenames",
                "synthesis_confidence",
            ],
        },
    }


def segments_schema() -> dict:
    """Per-flight-segment breakdown — only emitted by the airfare extractor.

    Bare array, same pattern as `nightly_rates_schema()`. Reduction
    uses entries to derive segment count and (eventually) total flight
    time. Each entry carries the minimum needed to display the
    multi-segment trip: flight_number, from/to IATA codes, and the
    segment's departure datetime. Per-segment airline + class can be
    added later if multi-airline trips become a real corpus pattern.
    """
    return {
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "flight_number": {
                    "type": "string",
                    "description": "Carrier code + number (e.g. 'UA1448', 'AI179').",
                },
                "from_airport": {
                    "type": "string",
                    "description": "IATA code of the segment's origin.",
                },
                "to_airport": {
                    "type": "string",
                    "description": "IATA code of the segment's destination.",
                },
                "departure_datetime": {
                    "type": "string",
                    "description": "ISO 8601 local departure datetime (e.g. '2025-11-23T10:30').",
                },
            },
            "required": [
                "flight_number",
                "from_airport",
                "to_airport",
                "departure_datetime",
            ],
        },
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


def extras_block_schema(
    include_nightly_rates: bool = False,
    include_segments: bool = False,
) -> dict:
    """Per-receipt extras — extracted from the receipt but NOT submitted to
    the FA portal. Reduction reads these to derive schema fields the model
    can't know in isolation (foreign vs domestic, original currency, FX,
    daily_rate average for lodging, segment count for airfare).
    See the multi-call architecture in docs/internals.md §5.

    `include_nightly_rates`: only the lodging kind needs the per-night
    breakdown — meal and transport pass through with the merchant_address
    + printed_currency pair only.

    `include_segments`: only the airfare kind needs the per-flight-segment
    breakdown.
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
    if include_segments:
        properties["segments"] = segments_schema()
        required.append("segments")
    return {
        "type": "object",
        "properties": properties,
        "required": required,
    }


def transaction_line_schema(
    *,
    expense_type_values: list[str] | None = None,
    detail_block_name: str | None = None,
    detail_block: dict | None = None,
    include_common: bool = True,
    include_detail: bool = True,
    include_extras: bool = True,
    include_nightly_rates: bool = False,
    include_segments: bool = False,
) -> dict:
    """One transaction line (or a sub-slice of one) as Gemini emits it.

    Single-call kinds (meal, transport) use the default — common +
    detail block + extras all included. Multi-call kinds (lodging,
    airfare) emit two schemas: a "main" with `include_extras=False`
    and an "extras" with `include_common=False, include_detail=False,
    include_nightly_rates=True` (lodging) or `include_segments=True`
    (airfare).

    The expense_kind discriminator was dropped (an earlier rework): the
    per-kind extractor router (`scripts/extract_<kind>.py`) knows the
    kind from the FA's upload-form choice — it doesn't need to be
    repeated inside the per-document JSON. The detail block (when
    present) is the structural discriminator instead.
    """
    properties: dict = {}
    required: list[str] = []
    if include_common:
        assert expense_type_values is not None, (
            "include_common requires expense_type_values"
        )
        properties["common"] = common_block_schema(expense_type_values)
        required.append("common")
    if include_detail:
        assert detail_block_name and detail_block, (
            "include_detail requires both detail_block_name and detail_block"
        )
        properties[detail_block_name] = detail_block
        required.append(detail_block_name)
    if include_extras:
        properties["extras"] = extras_block_schema(
            include_nightly_rates=include_nightly_rates,
            include_segments=include_segments,
        )
        required.append("extras")
    return {"type": "object", "properties": properties, "required": required}


# Each entry produces one `generated/response_schema_<name>.json` file.
# Single-call kinds (meal, transport) have one entry. Multi-call kinds
# (lodging) have multiple — one per Gemini call. The orchestration of
# the calls lives in `scripts/extract_<kind>.py`; this list just owns
# the schema files that get generated.
SCHEMAS_TO_GENERATE: list[tuple[str, dict]] = [
    # Meal: 2-call split. The tip-cap added
    # pre_tax_amount + tax_amount to meal_details and Vertex rejected
    # the schema with 400 INVALID_ARGUMENT (property-count ceiling).
    # Meal splits the same way lodging is split: main carries
    # common + meal_details (now expanded with the tax pair); extras
    # carries the extras block alone. The two calls run in parallel
    # in extract_meal.py and the dicts are merged before the per-doc
    # JSON is written.
    (
        "response_schema_meal_main.json",
        dict(
            expense_type_values=KIND_EXPENSE_TYPES["meal"],
            detail_block_name="meal_details",
            detail_block=meal_details_block_schema(),
            include_extras=False,
        ),
    ),
    (
        "response_schema_meal_extras.json",
        dict(
            include_common=False,
            include_detail=False,
        ),
    ),
    # Transport: 2-call split, same reason as meal. The tip-cap
    # added tip_amount + pre_tax_amount + tax_amount to
    # ground_transport_details, pushing past the property-count
    # ceiling. Split follows the lodging pattern; the tax/tip
    # fields ride in transport_main.
    (
        "response_schema_transport_main.json",
        dict(
            expense_type_values=KIND_EXPENSE_TYPES["transport"],
            detail_block_name="ground_transport_details",
            detail_block=ground_transport_details_block_schema(),
            include_extras=False,
        ),
    ),
    (
        "response_schema_transport_extras.json",
        dict(
            include_common=False,
            include_detail=False,
        ),
    ),
    # Lodging is split across two parallel Gemini calls (see
    # extract_lodging.py). Each call's schema must fit under
    # Vertex's property-count ceiling (~5 detail-block leaves with
    # inlined _meta in practice). The "main" call extracts
    # common + lodging_details; the
    # "extras" call extracts the extras block with the per-night rate
    # breakdown. The dicts are merged in extract_lodging.py to produce
    # the same per-doc JSON shape a single call would have produced.
    (
        "response_schema_lodging_main.json",
        dict(
            expense_type_values=KIND_EXPENSE_TYPES["lodging"],
            detail_block_name="lodging_details",
            detail_block=lodging_details_block_schema(),
            include_extras=False,
        ),
    ),
    (
        "response_schema_lodging_extras.json",
        dict(
            include_common=False,
            include_detail=False,
            include_nightly_rates=True,
        ),
    ),
    # Airfare: 3-call split (lodging used 2; airfare needs 3 because its
    # detail block has 9 leaves vs lodging's 6). The probe with all 9
    # detail leaves in `airfare_main` returned a 400 from Vertex
    # (17 total leaves at common 8 + detail 9, over the ceiling).
    # Splitting airfare_details across two calls (main: 5 flight-centric
    # fields; aux: 4 booking/payment-centric fields) keeps each call
    # comfortably under the limit. Both emit under the SAME
    # `airfare_details` key so the extractor can 1-deep-merge them
    # before writing the per-doc JSON. Reduction sees the union as a
    # single complete object — same shape a hypothetical single-call
    # would have produced.
    (
        "response_schema_airfare_main.json",
        dict(
            expense_type_values=KIND_EXPENSE_TYPES["airfare"],
            detail_block_name="airfare_details",
            detail_block=airfare_details_block_schema(AIRFARE_FLIGHT_FIELDS),
            include_extras=False,
        ),
    ),
    (
        "response_schema_airfare_aux.json",
        dict(
            detail_block_name="airfare_details",
            detail_block=airfare_details_block_schema(AIRFARE_BOOKING_FIELDS),
            include_common=False,
            include_extras=False,
        ),
    ),
    (
        "response_schema_airfare_extras.json",
        dict(
            include_common=False,
            include_detail=False,
            include_segments=True,
        ),
    ),
    # Conference registration: 5 T3 detail leaves at the single-call
    # ceiling. The receipt extractor produces a transaction line in the
    # standard pattern. Supporting docs and the synthesis-bundle output
    # use their own (non-transaction-line) schemas — written separately
    # in main() below since they don't fit transaction_line_schema.
    (
        "response_schema_conference_registration.json",
        dict(
            expense_type_values=KIND_EXPENSE_TYPES["conference_registration"],
            detail_block_name="conference_registration_details",
            detail_block=conference_registration_details_block_schema(),
        ),
    ),
    # Miscellaneous (posters, printing, etc.): no kind-specific detail
    # block — the common + extras blocks carry everything we need.
    # `expense_type` is restricted to `other_business_expense` so
    # Gemini can't accidentally re-route a poster as e.g. car_rental.
    (
        "response_schema_miscellaneous.json",
        dict(
            expense_type_values=KIND_EXPENSE_TYPES["miscellaneous"],
            include_detail=False,
        ),
    ),
    # Membership: professional society / conference org dues. Same
    # shape as miscellaneous (no detail block); enum restriction
    # ensures Gemini emits membership_dues, which the CSV mappers
    # render as "Membership Dues" / "Membership Dues - Foreign".
    (
        "response_schema_membership.json",
        dict(
            expense_type_values=KIND_EXPENSE_TYPES["membership"],
            include_detail=False,
        ),
    ),
    # Personal mileage. Single-call: mileage_details has only 4
    # T3 leaves, well under the property-count ceiling. Extras
    # block included (merchant_address null for mileage,
    # printed_currency null since IRS rate is USD-denominated).
    (
        "response_schema_mileage.json",
        dict(
            expense_type_values=KIND_EXPENSE_TYPES["mileage"],
            detail_block_name="mileage_details",
            detail_block=mileage_details_block_schema(),
        ),
    ),
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

    # Standard transaction-line schemas (meal, transport, lodging,
    # airfare, conference_registration).
    for filename, kwargs in SCHEMAS_TO_GENERATE:
        line_schema = transaction_line_schema(**kwargs)
        response_schema = {"type": "array", "items": line_schema}
        out_path = OUTPUT_DIR / filename
        out_path.write_text(json.dumps(response_schema, indent=2) + "\n")
        print(f"wrote {out_path.relative_to(REPO_ROOT)}")

    # Non-transaction-line schemas: supporting conference docs
    # (no `common` block — they're context, not expenses) and the
    # cross-document synthesis output.
    for filename, factory in (
        (
            "response_schema_supporting_conference_doc.json",
            supporting_conference_doc_response_schema,
        ),
        (
            "response_schema_synthesis_conference_bundle.json",
            synthesis_conference_bundle_response_schema,
        ),
    ):
        response_schema = factory()
        out_path = OUTPUT_DIR / filename
        out_path.write_text(json.dumps(response_schema, indent=2) + "\n")
        print(f"wrote {out_path.relative_to(REPO_ROOT)}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
