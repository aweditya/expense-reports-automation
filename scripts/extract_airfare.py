#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the airfare expense kind.

Three parallel Gemini calls per ticket:

- "main" call:    extracts `common` + the flight-centric fields of
                  `airfare_details` (airline, departure_airport,
                  destination_airport, class_of_ticket, round_trip).
- "aux" call:     extracts the booking-centric fields of
                  `airfare_details` (travelers_name, ticket_number,
                  ticket_amount, booking_method).
- "extras" call:  extracts the `extras` block — printed currency
                  (merchant_address is N/A for airfare) and the
                  per-segment flight breakdown (`segments[]`) that
                  reduction uses to derive segment count + total
                  flight time.

All three calls receive the full ticket; only the response schema
differs. The split is forced by Vertex's empirical schema
property-count ceiling: the airfare detail block has too many
leaves for a single call.

Results merge via a 1-deep dict union. Top-level keys from all
three calls combine; the `airfare_details` dicts from the main and
aux calls are unioned. The merged per-doc JSON matches the
single-call shape; reduction sees no difference.

FX handling: for non-USD tickets, `common.line_amount_usd` is left
null (origin: `needs_fx_conversion`). `airfare_details.ticket_amount`,
`common.original_currency`, and `common.original_amount` carry the
printed values. Reduction applies the FX rate.

Dispatched by `local_app_simple.py` based on the per-file kind
choice in the upload form.
"""

from __future__ import annotations

import json
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from evidence_bbox import (
    format_tokens_for_prompt,
    ocr_document,
    populate_bboxes,
)
from extractor_lib import (
    GeminiCallFailed,
    detect_mime_type,
    parse_args,
    single_call,
)


REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH_MAIN = REPO_ROOT / "generated" / "response_schema_airfare_main.json"
SCHEMA_PATH_AUX = REPO_ROOT / "generated" / "response_schema_airfare_aux.json"
SCHEMA_PATH_EXTRAS = REPO_ROOT / "generated" / "response_schema_airfare_extras.json"


# Identical _meta convention across all three calls. Defined once so
# the prompts can't drift out of sync. Two airfare-specific origin
# values added to the existing list:
#   - `needs_fx_conversion`: line_amount_usd left null on foreign
#     tickets; reduction's mock FX fills it.
#   - `not_applicable_for_airfare`: merchant_address slot — airfare
#     doesn't have a merchant address (the airline name lives in
#     airfare_details, the route in extras.segments).
META_CONVENTION = """# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous
  but defensible, `high` when unambiguous on the ticket. NOT a
  probability.
- `confidence_reason` is a short justification for the confidence
  level. REQUIRED for every leaf — both as FA-facing context AND as
  a debugging signal for whoever audits the extraction.
  - For `high` confidence: ≤5 words. "Airline in header.",
    "Total clearly printed.", "USD literal on ticket."
  - For `medium` and `low`: ≤15 words. Explain the ambiguity.
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, an exact `quote` from the ticket, AND `token_ids: [N, N, ...]`
 . `token_ids` are integer indices from the numbered
  Document AI token list appended at the bottom of this prompt — pick
  the IDs of the tokens whose printed text covers your `quote`. The
  concatenated text of those tokens should match (or closely paraphrase)
  the quote string. Use a continuous global ID range across all pages
  (page boundaries are invisible in the token list). When the quote
  spans multiple non-adjacent regions (e.g. "MUMBAI ... SAN FRANCISCO"
  joining the origin + destination), include the IDs of ALL the
  relevant tokens — they get unioned into one bounding box. If you
  genuinely can't identify which tokens cover the quote, omit
  `token_ids` and the post-pass will fall back to text matching; do
  NOT invent IDs.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`,
  `not_applicable_for_foreign`, `not_applicable_for_transport`,
  `not_applicable_for_airfare`, or `needs_fx_conversion`.
  Do NOT cite an unrelated quote with `document_span` to evidence
  a null value. `token_ids` MUST be omitted (or empty) for
  non-`document_span` evidence.
- `needs_review` is true for any value you guessed, and any field
  where you used `medium` or `low` confidence.
- `flags` stays empty unless you observe something irregular
  (e.g. refund/exchange artifacts, ancillary fees beyond the base fare).
"""


PROMPT_MAIN = f"""You are extracting receipt-level facts and flight-itinerary
fields from an airfare ticket for a Stanford expense report. The ticket
is attached as an image or PDF (e-tickets are typically 1-6 pages;
real conference travel often has 2-4 segments per ticket).

Return a JSON array containing one transaction line object with
`common` and `airfare_details` blocks. Two parallel calls (with
different schemas) are separately extracting the booking-side fields
(travelers_name, ticket_number, ticket_amount, booking_method) and the
extras block (printed_currency + per-segment breakdown) — focus only
on what's in your schema here. Shape is enforced by the response
schema; fill the values from the ticket.

# Reasoning rules

`expense_type`:
- `airfare_domestic` when ALL flight segments stay within the United States.
- `airfare_foreign` when ANY segment crosses an international border
  (one-way BOM→SFO is foreign; round-trip SFO→ORD is domestic).
- The schema only accepts these two values for airfare receipts.

`airfare_details.airline`:
- The carrier name as it appears on the ticket (e.g. "United Airlines",
  "Air India", "Southwest Airlines"). Use the brand name from the
  ticket header / logo. For multi-airline trips (codeshare), use the
  marketing carrier on the first segment.

`airfare_details.departure_airport`:
- IATA 3-letter code of the trip's ORIGIN (e.g. "SFO", "BOM").
  For round-trips this is where the trip starts AND ends.

`airfare_details.destination_airport`:
- IATA 3-letter code of the FURTHEST destination (the turn-around
  point for round-trips, or the final destination for one-ways).
  E.g. "PIT" for SFO→PIT→SFO, "ORD" for SFO→ORD→SFO, "SFO" for
  one-way BOM→SFO.

`airfare_details.class_of_ticket`:
- Map the printed cabin class to one of: `coach`, `premium_economy`,
  `business`, `first`. Common mappings:
  - "Economy", "Main Cabin", "Basic", "Saver", `H`/`Y`/`B`/`M`/`S`
    fare basis → `coach`
  - "Premium Economy", "Comfort+", "Economy Plus", "Extra Comfort"
    → `premium_economy`
  - "Business", "BusinessFirst" → `business`
  - "First", "International First", `F`/`A` fare basis → `first`
  - If the gray zone is unclear, default to `coach` with `medium`
    confidence and note the printed name in `confidence_reason`.

`airfare_details.round_trip`:
- `true` if the itinerary returns to the starting airport.
- `false` for one-way tickets.
- Verify by checking that the LAST segment's destination matches the
  FIRST segment's origin.

`country_of_activity`:
- The country of the FURTHEST destination. "United States" for
  domestic round-trips. The destination country for one-ways and
  international round-trips (e.g. "United States" for BOM→SFO since
  that's the trip's purpose-end).

`original_currency` and `original_amount`:
- For USD tickets: BOTH null with `kind: system_generated, origin:
  not_applicable_for_domestic`.
- For non-USD tickets (INR, EUR, GBP, JPY, etc.): fill them.
  `original_amount` is the total as printed on the ticket in that
  currency; `original_currency` is the ISO 4217 code.

`line_amount_usd`:
- For USD tickets: fill with the printed total. `high` confidence.
- For non-USD tickets: set value to null with `kind: system_generated,
  origin: needs_fx_conversion`. Reduction will compute the USD amount
  from `airfare_details.ticket_amount` + `original_currency` using a
  mocked FX rate. DO NOT attempt the conversion here — the model
  doesn't have a reliable rate.

`foreign_activity_type`:
- For domestic: null with `kind: system_generated, origin:
  not_applicable_for_domestic`.
- For foreign: `conference` if the ticket clearly references a
  conference (group code / conference name visible). Otherwise
  `other` (most personal-corpus tickets won't say).

`remarks`:
- One short sentence: "{{class}} {{airline}} {{route}} on {{date}}"
  (e.g. "Coach United round-trip SFO↔ORD on 2025-11-23",
  "Coach Air India one-way BOM→SFO on 2024-09-02"). Note any seat
  fees / EMDs / fare-class peculiarities here as a free-text capture.

{META_CONVENTION}"""


PROMPT_AUX = f"""You are extracting booking-side fields from an airfare
ticket for a Stanford expense report. The ticket is attached as an
image or PDF.

Two parallel calls (with different schemas) are separately extracting
the receipt-level facts + flight-itinerary fields (date, totals,
expense_type, airline, route, class) and the extras block (printed
currency + per-segment breakdown). Your job is the **booking-side
fields of `airfare_details` only**: traveler name, ticket number,
ticket amount, and booking method. Return a JSON array containing one
transaction line object with only the `airfare_details` block
populated. Shape is enforced by the response schema.

# Reasoning rules

`airfare_details.travelers_name`:
- The name of the person FLYING (the passenger), not the booker if
  they differ. Most tickets show only one passenger. Format as it
  appears on the ticket — typically "LASTNAME/FIRSTNAME" or
  "FIRSTNAME LASTNAME". If multiple passengers are listed, use the
  primary passenger.

`airfare_details.ticket_number`:
- The full ticket number (e.g. "0162342580940", "098 2168766518").
  Tickets are typically 13 digits; some carriers (Air India,
  Lufthansa) print them with a 3-digit prefix and a space. Strip
  spaces if you want, but include all digits.

`airfare_details.ticket_amount`:
- The total amount as printed on the ticket, in WHATEVER currency
  the ticket prints. For a USD ticket, this matches the USD total.
  For a non-USD ticket (INR, EUR, etc.), this is the foreign-currency
  total — reduction handles the FX conversion downstream from this
  field. Always fill, regardless of currency.

`airfare_details.booking_method`:
- `stanford_travel_egencia` only if "Egencia" appears on the ticket.
- `stanford_travel_key_travel` only if "Key Travel" appears.
- `stanford_travel_connect_ua` / `_dl` / `_aa` / `_as` / `_ha` only if
  the ticket explicitly mentions Stanford Travel + the matching
  carrier (United / Delta / American / Alaska / Hawaiian).
- `other` for everything else — most direct-to-airline bookings, all
  personal-corpus tickets, third-party booking tools (Expedia, Google
  Flights, etc.). Default to `other` when unsure; use `medium`
  confidence in that case.

{META_CONVENTION}"""


PROMPT_EXTRAS = f"""You are extracting auxiliary signals from an airfare
ticket for a Stanford expense report. The ticket is attached as an
image or PDF (e-tickets are typically 1-6 pages).

Two parallel calls (with different schemas) are separately extracting
the receipt-level facts + flight fields and the booking-side fields.
Your job is the **`extras` block only** — the printed currency and
the per-flight-segment breakdown. Return a JSON array containing one
transaction line object with only the `extras` block populated.
Shape is enforced by the response schema.

# Reasoning rules

`extras.merchant_address`:
- Set value to null with `kind: system_generated, origin:
  not_applicable_for_airfare`. Airline tickets don't have a merchant
  address in the same sense as a restaurant or hotel — the airline
  name lives in `airfare_details.airline` and the route is captured
  per-segment in `extras.segments[]`.

`extras.printed_currency`:
- The currency literally on the ticket. ISO 4217 code if you see one
  (USD, INR, EUR, GBP, JPY). Otherwise infer from a printed symbol
  (`$` is most likely USD; `₹` is INR; `€` is EUR; `£` is GBP; `¥`
  is JPY; use `medium` confidence and let reduction disambiguate).

`extras.segments`:
- This is the key airfare-specific extraction. Emit ONE entry per
  flight segment in the itinerary (a one-way ticket has 1+ segments,
  a round-trip has 2+).
- For a typical SFO↔ORD round-trip: 2 segments (outbound + return).
- For a connection like SFO→ORD→FRA + return FRA→MUC + MUC→ORD→SFO:
  5 segments.
- `flight_number` is carrier code + number (e.g. "UA1448", "AI179",
  "WN4184"). Combine the IATA carrier code with the flight number
  if they're printed separately.
- `from_airport` and `to_airport` are IATA 3-letter codes of the
  segment's origin and destination.
- `departure_datetime` is ISO 8601 LOCAL time at the departure
  airport (e.g. "2025-11-23T10:30"). No timezone offset — the
  airport's local time is what the ticket prints. Date alone (no
  time) is acceptable when the ticket only shows the day; use
  "YYYY-MM-DDT00:00" in that case with `medium` confidence.

The `segments` array itself is bare — no `_meta` block carries
confidence; reduction signals uncertainty downstream by leaving the
array empty if the itinerary couldn't be recovered.

{META_CONVENTION}"""


def merge_airfare_lines(
    main_result: list,
    aux_result: list,
    extras_result: list,
) -> list:
    """Merge the three single-call outputs into one transaction line.

    Each call returns a JSON array with exactly one transaction line
    object. The three top-level dicts have these key sets:

      main:    {common, airfare_details: {flight fields}}
      aux:     {airfare_details: {booking fields}}
      extras:  {extras}

    `main` and `aux` BOTH emit under the `airfare_details` key with
    disjoint subsets of fields, so a shallow union would clobber one
    side. The merge is 1-deep on `airfare_details` only — top-level
    keys union normally; `airfare_details` is itself unioned. Result
    has the same shape a hypothetical single-call extraction would
    have produced (full common + full airfare_details + extras).
    """
    for label, result in (("main", main_result), ("aux", aux_result), ("extras", extras_result)):
        if not (isinstance(result, list) and len(result) == 1):
            raise ValueError(
                f"{label} result not a single-element array: {result!r}"
            )
        if not isinstance(result[0], dict):
            raise ValueError(f"{label} result element not a dict: {result[0]!r}")

    main_line = main_result[0]
    aux_line = aux_result[0]
    extras_line = extras_result[0]

    # Top-level union: common (from main) + extras (from extras_line).
    # airfare_details from main is in here too; we'll override with the
    # 1-deep union below.
    merged = {**main_line, **extras_line}

    # 1-deep merge on airfare_details: union the field-subset dicts
    # from the main and aux calls into one complete airfare_details.
    main_details = main_line.get("airfare_details", {})
    aux_details = aux_line.get("airfare_details", {})
    if not (isinstance(main_details, dict) and isinstance(aux_details, dict)):
        raise ValueError(
            "airfare_details must be a dict in both main and aux results"
        )
    merged["airfare_details"] = {**main_details, **aux_details}

    return [merged]


def main() -> int:
    args = parse_args(__doc__)

    if not args.image.exists():
        sys.exit(f"image not found: {args.image}")
    if not args.project:
        sys.exit(
            "project required: pass --project or set $VERTEX_PROJECT_ID. "
            "Cloud Run gets this from --set-env-vars in deploy/cloudbuild.yaml."
        )

    from google import genai

    # Build the client + load the document once; all three Gemini calls
    # share them. The google-genai SDK's client is thread-safe (httpx
    # under the hood), so three concurrent generate_content calls are fine.
    client = genai.Client(
        vertexai=True, project=args.project, location=args.location
    )
    image_bytes = args.image.read_bytes()
    mime = detect_mime_type(args.image)

    #: Document AI runs ONCE per receipt, BEFORE the
    # parallel Gemini calls. Its tokens get appended to ALL THREE
    # prompts so every parallel call cites the same global token-id
    # range. Failure here is non-fatal — extraction proceeds without
    # token-id grounding; populate_bboxes falls back to text matching
    # at write time.
    doc = None
    try:
        doc = ocr_document(args.image)
    except Exception as err:
        print(
            f"warning: Document AI OCR failed for {args.image}: {err}; "
            "airfare extraction will proceed without token-id grounding.",
            file=sys.stderr,
        )

    prompt_main = PROMPT_MAIN
    prompt_aux = PROMPT_AUX
    prompt_extras = PROMPT_EXTRAS
    if doc is not None:
        token_list_text, _, _ = format_tokens_for_prompt(doc)
        suffix = (
            "\n\n# Numbered Document AI tokens (for `token_ids` grounding)\n\n"
            + token_list_text
        )
        prompt_main = PROMPT_MAIN + suffix
        prompt_aux = PROMPT_AUX + suffix
        prompt_extras = PROMPT_EXTRAS + suffix

    shared_call_kwargs = dict(
        client=client,
        model=args.model,
        image_filename=args.image.name,
        image_bytes=image_bytes,
        mime=mime,
        output_base=args.output,
    )

    # Three threads, three parallel Gemini calls. Total wall time ≈ max
    # of the three call times, not the sum. If any call raises, we
    # surface the error and exit non-zero — partial extractions don't
    # produce a per-doc JSON.
    with ThreadPoolExecutor(max_workers=3) as executor:
        future_main = executor.submit(
            single_call,
            prompt=prompt_main,
            response_schema_path=SCHEMA_PATH_MAIN,
            diag_label="main",
            **shared_call_kwargs,
        )
        future_aux = executor.submit(
            single_call,
            prompt=prompt_aux,
            response_schema_path=SCHEMA_PATH_AUX,
            diag_label="aux",
            **shared_call_kwargs,
        )
        future_extras = executor.submit(
            single_call,
            prompt=prompt_extras,
            response_schema_path=SCHEMA_PATH_EXTRAS,
            diag_label="extras",
            **shared_call_kwargs,
        )
        try:
            result_main = future_main.result()
            result_aux = future_aux.result()
            result_extras = future_extras.result()
        except GeminiCallFailed as err:
            print(err, file=sys.stderr)
            return 1

    merged = merge_airfare_lines(result_main, result_aux, result_extras)
    # Inject source_filename + populate bbox grounding. Pass the
    # already-OCR'd doc so populate_bboxes (dual-path) doesn't call
    # Document AI a second time.
    for entry in merged:
        if isinstance(entry, dict):
            entry["source_filename"] = args.image.name
            populate_bboxes(entry, args.image, doc=doc)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(merged, indent=2, ensure_ascii=False))
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
