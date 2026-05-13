#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the lodging expense kind.

Two parallel Gemini calls per folio:

- "main" call:    extracts `common` + `lodging_details` (receipt-level
                  facts: date, total, hotel name, dates, etc.).
- "extras" call:  extracts the `extras` block — merchant address,
                  printed currency, and the per-night rate breakdown
                  that reduction averages into `lodging_details.daily_rate`.

Both calls see the **full folio**; they differ only in their response
schema. The multi-call pattern is necessary because Vertex's Schema
validator rejects schemas that have > ~5 detail-block leaves with
inlined `_meta` — see `docs/redesign-regrets.md` 2026-05-13. Meal and
transport stay single-call because their detail blocks are smaller.

After both calls return, their single-element output arrays' first
dicts are merged into the same `{common, lodging_details, extras}`
shape a single call would have produced. Reduction (Rust) sees no
difference from the single-call kinds.

Phase 3 v1 focuses on English-language folios — clean PDFs from US
hotel chains (Hilton/Hampton/Hyatt/Sheraton/Homewood/Hyatt Place) that
are the bulk of the corpus the FA submits today. Multilingual support
(French/German/Japanese folios) is a v2 follow-up.

Dispatched by `local_app_simple.py` based on the FA's per-file kind
choice in the upload form.
"""

from __future__ import annotations

import json
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from extractor_lib import (
    GeminiCallFailed,
    detect_mime_type,
    parse_args,
    single_call,
)


REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH_MAIN = REPO_ROOT / "generated" / "response_schema_lodging_main.json"
SCHEMA_PATH_EXTRAS = REPO_ROOT / "generated" / "response_schema_lodging_extras.json"


# Identical _meta convention across both calls. Defined once so the two
# prompts can't drift out of sync.
META_CONVENTION = """# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous
  but defensible, `high` when unambiguous on the folio. NOT a
  probability.
- `confidence_reason` is a short justification for the confidence
  level. REQUIRED for every leaf — both as FA-facing context AND as
  a debugging signal for whoever audits the extraction.
  - For `high` confidence: ≤5 words. "Hotel name in header.",
    "Arrival date printed clearly.", "USD literal on folio."
  - For `medium` and `low`: ≤15 words. Explain the ambiguity.
    Examples:
      medium: "Checkout date inferred from departure label, not standalone."
      low: "Folio scan blurry around room rate; best-effort read."
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, and an exact `quote` from the folio.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`,
  `not_applicable_for_foreign`, or `not_applicable_for_transport`.
  Do NOT cite an unrelated quote with `document_span` to evidence
  a null value.
- `needs_review` is true for any value you guessed, and any field
  where you used `medium` or `low` confidence.
- `flags` stays empty unless you observe something irregular
  (e.g. resort fee out of policy, room category mismatch with rate).
"""


PROMPT_MAIN = f"""You are extracting receipt-level facts from a hotel
folio for a Stanford expense report. The folio is attached as an image
or PDF (folios are typically multi-page; later pages often itemize
nightly charges).

Return a JSON array containing one transaction line object with
`common` and `lodging_details` blocks. A parallel call to you (with a
different schema) is separately extracting the `extras` block — focus
only on what's in your schema here. Shape is enforced by the response
schema; fill the values from the folio.

# Reasoning rules

`expense_type`:
- `lodging_domestic` when the hotel address is in the United States.
- `lodging_foreign` when the hotel is outside the US.
- The schema only accepts these two values for lodging receipts.

`lodging_details.hotel_name`:
- The brand name + location qualifier as it appears on the folio
  (e.g. "Hilton San Francisco Airport", "Hampton by Hilton Munich
  North", "Hyatt Place Las Vegas"). Don't include rate plan codes or
  loyalty-program names.

`lodging_details.location`:
- "City, State" for US hotels (e.g. "San Francisco, CA"), or "City,
  Country" for foreign (e.g. "Munich, Germany"). Pull from the hotel
  address printed on the folio.

`lodging_details.check_in_date` and `lodging_details.check_out_date`:
- ISO 8601 (YYYY-MM-DD). Hotels usually print these as "Arrival" /
  "Departure" near the top. The dates from the **stay**, not the
  date the folio was printed.

`lodging_details.booking_method`:
- `conference_hotel` if the folio mentions the hotel as the
  conference venue / has a conference group code.
- `stanford_travel_egencia` only if "Egencia" appears on the folio.
- `stanford_travel_key_travel` only if "Key Travel" appears.
- Otherwise `other` (the default for almost every folio — Stanford
  doesn't print "Stanford Travel" on most hotel folios).

`lodging_details.is_shared_lodging`:
- `false` is the default. Set `true` only if the folio explicitly
  shows two guests with different last names sharing the room
  (e.g. "Guest 1: Smith, Guest 2: Jones").

`country_of_activity`:
- The country of the hotel address. "United States" for US folios.

`original_currency` and `original_amount`:
- For USD folios (printed amounts in dollars), set BOTH to null with
  `kind: system_generated, origin: not_applicable_for_domestic`.
- For non-USD folios (EUR, JPY, GBP, etc.), fill them. `original_amount`
  is the total as printed on the folio in that currency;
  `line_amount_usd` is the converted USD amount.

`foreign_activity_type`:
- For domestic: null with `kind: system_generated, origin:
  not_applicable_for_domestic`.
- For foreign: `conference` if the hotel hosted the trip's
  conference, otherwise `other`.

`remarks`:
- One short sentence: "{{nights}}-night stay at {{hotel}} in {{city}}
  from {{check_in}} to {{check_out}}."

{META_CONVENTION}"""


PROMPT_EXTRAS = f"""You are extracting auxiliary signals from a hotel
folio for a Stanford expense report. The folio is attached as an image
or PDF (folios are typically multi-page; later pages often itemize
nightly charges).

A parallel call to you (with a different schema) is separately
extracting the receipt-level facts. Your job is the **`extras` block
only** — the merchant address, the printed currency, and the per-night
rate breakdown. Return a JSON array containing one transaction line
object with only the `extras` block populated. Shape is enforced by
the response schema.

# Reasoning rules

`extras.merchant_address`:
- The full printed hotel address (street + city + region + country).
  Reduction parses this for the country signal.

`extras.printed_currency`:
- Same rule as other receipts. ISO 4217 if printed (USD/EUR/JPY/GBP),
  else inferred from a printed symbol with `medium` confidence.

`extras.nightly_rates`:
- This is the key lodging-specific extraction. Emit ONE entry per
  night of the stay (i.e. `check_out_date - check_in_date` entries).
- `date` is the night's date in ISO 8601. The first night's date
  matches the folio's check-in date. The last entry is the night
  BEFORE the check-out date (you don't pay for the checkout day's
  night).
- `rate` is the room rate that night, in the printed currency
  (matches `extras.printed_currency`).
- `taxes_and_fees` is the sum of all taxes/fees applied that night
  (VAT, occupancy tax, city tax, resort fee, etc.). Use `0.0` if
  the folio doesn't break taxes out per-night.
- If the folio shows a FLAT rate (same every night), still emit one
  entry per night with the flat rate repeated. Reduction averages
  them — flat rates trivially average to themselves.
- If the folio shows ONLY a total without per-night breakdown, emit
  a single entry per night with `rate = total / number_of_nights`
  and `taxes_and_fees = 0.0`. The `nightly_rates` array itself is
  bare — no `_meta` block carries confidence; reduction signals
  uncertainty downstream by leaving `lodging_details.daily_rate`
  unset if the array is empty.

{META_CONVENTION}"""


def merge_transaction_lines(main_result: list, extras_result: list) -> list:
    """Merge the two single-call outputs into one transaction line.

    Each call returns a JSON array with exactly one transaction line
    object (the response schema enforces `type: array`, and the prompt
    asks for one line). The two arrays' first elements have disjoint
    top-level keys — `main = {common, lodging_details}` and
    `extras = {extras}` — so a dict merge yields the same per-doc JSON
    shape a single-call extraction would have produced.
    """
    if not (isinstance(main_result, list) and len(main_result) == 1):
        raise ValueError(
            f"main result not a single-element array: {main_result!r}"
        )
    if not (isinstance(extras_result, list) and len(extras_result) == 1):
        raise ValueError(
            f"extras result not a single-element array: {extras_result!r}"
        )
    main_line = main_result[0]
    extras_line = extras_result[0]
    if not (isinstance(main_line, dict) and isinstance(extras_line, dict)):
        raise ValueError("merge expects dict elements")
    return [{**main_line, **extras_line}]


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

    # Build the client + load the document once; both Gemini calls share
    # them. The google-genai SDK's client is thread-safe (httpx under
    # the hood), so two concurrent generate_content calls are fine.
    client = genai.Client(
        vertexai=True, project=args.project, location=args.location
    )
    image_bytes = args.image.read_bytes()
    mime = detect_mime_type(args.image)

    shared_call_kwargs = dict(
        client=client,
        model=args.model,
        image_filename=args.image.name,
        image_bytes=image_bytes,
        mime=mime,
        output_base=args.output,
    )

    # Two threads, two parallel Gemini calls. Total wall time ≈ max of
    # the two call times, not the sum. If either call raises, we surface
    # the error and exit non-zero — partial extractions don't produce a
    # per-doc JSON.
    with ThreadPoolExecutor(max_workers=2) as executor:
        future_main = executor.submit(
            single_call,
            prompt=PROMPT_MAIN,
            response_schema_path=SCHEMA_PATH_MAIN,
            diag_label="main",
            **shared_call_kwargs,
        )
        future_extras = executor.submit(
            single_call,
            prompt=PROMPT_EXTRAS,
            response_schema_path=SCHEMA_PATH_EXTRAS,
            diag_label="extras",
            **shared_call_kwargs,
        )
        try:
            result_main = future_main.result()
            result_extras = future_extras.result()
        except GeminiCallFailed as err:
            print(err, file=sys.stderr)
            return 1

    merged = merge_transaction_lines(result_main, result_extras)
    # Inject source_filename on the merged line — same as run_extraction
    # does for single-call kinds.
    for entry in merged:
        if isinstance(entry, dict):
            entry["source_filename"] = args.image.name

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(merged, indent=2, ensure_ascii=False))
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
