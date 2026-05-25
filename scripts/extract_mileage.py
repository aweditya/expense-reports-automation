#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the personal_mileage expense kind.

Single-call: mileage_details has 4 T3 leaves which is well under
Vertex's schema-property ceiling, so we use the simple run_extraction
helper (like miscellaneous + membership) rather than the split-call
pattern (meal/transport/lodging/airfare).

The receipt is typically a Google Maps screenshot showing a route
with the distance + addresses, or a driving log table. The IRS
Standard Mileage Rate gets applied by reduction at line_amount_usd
computation time (reduce reads generated/irs_mileage_rates.json,
multiplies by the trip year's business rate). The extractor itself
just pulls the distance + endpoints from the receipt.

Dispatched by `local_app_simple.py` based on the FA's per-file kind
choice (`mileage`) in the upload form.
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_extraction


RESPONSE_SCHEMA_PATH = (
    Path(__file__).resolve().parent.parent / "generated" / "response_schema_mileage.json"
)


PROMPT = """You are extracting a single personal-mileage transaction line
from a route screenshot or driving log for a Stanford expense report.
The receipt is attached as an image (Google Maps screenshot, a driving
log table, or an FA-generated trip summary).

Return a JSON array containing one transaction line object. Its shape
is enforced by the response schema — fill the values from the receipt.

# Reasoning rules

`expense_type`:
- Always `personal_mileage`. The schema only accepts this value for
  mileage receipts (the dispatcher routed the file here based on the
  FA's kind choice).

`mileage_details.distance_miles`:
- The total trip distance the FA is claiming, in MILES. Most Google
  Maps screenshots print this prominently (e.g. "32.4 mi", "45 miles").
- If the screenshot shows kilometers, convert to miles (1 km =
  0.621371 mi) and use `medium` confidence with a confidence_reason
  noting the conversion.
- If the receipt shows both one-way and round-trip distances, prefer
  the one the FA explicitly labels (often "Round trip: X mi"). When
  ambiguous, use the larger value and set confidence to `medium`.

`mileage_details.origin` / `mileage_details.destination`:
- The starting and ending addresses, as printed on the map or log.
  Use the full address when visible (street + city + state). When
  only a place name appears ("Stanford University", "SFO Airport"),
  use that.
- For round-trip routes shown as A → B (no return arrow), still treat
  A as origin and B as destination — the round-trip nature is
  captured in distance_miles, not by repeating addresses.

`mileage_details.trip_date`:
- The date the trip was taken, ISO 8601 (YYYY-MM-DD). On Google Maps
  screenshots this is sometimes printed as part of the screenshot
  metadata or in a date stamp; on driving logs it's the row's date
  column. If the date is not visible on the receipt itself, use null
  with `kind: system_generated, origin: not_present_in_receipt` —
  reduction will surface this as a needs_review issue and the FA
  will fill it via click-to-edit.

`country_of_activity`:
- The country of the origin (and destination — they're typically the
  same for personal mileage). "United States" for US trips.
- Personal mileage outside the US is rare; if both endpoints are
  foreign, set the foreign country.

`original_currency` / `original_amount`:
- ALWAYS null for personal_mileage: `kind: system_generated,
  origin: not_applicable_for_domestic`. The IRS rate is denominated
  in USD; there's no foreign-currency angle for mileage reimbursement.

`foreign_activity_type`:
- For US trips: null with `kind: system_generated, origin:
  not_applicable_for_domestic`.
- For foreign trips: pick from conference / research_collaboration /
  fieldwork / other based on context.

`remarks`:
- One short sentence: "{{distance}} mi from {{origin}} to {{destination}}
  on {{date}}" — e.g. "32 mi from Stanford to SFO on April 12."

`extras.merchant_address`:
- Set value to null with `kind: system_generated,
  origin: not_applicable_for_transport`. There's no merchant; mileage
  is FA-self-claimed, not a paid transaction with a vendor.

`extras.printed_currency`:
- ALWAYS null for mileage with `kind: system_generated,
  origin: not_applicable_for_domestic`. Reimbursement is USD via IRS
  rate; the receipt itself has no currency amount to print.

# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous
  but defensible, `high` when unambiguous on the receipt. NOT a
  probability.
- `confidence_reason` is a short justification for the confidence level.
  REQUIRED for every leaf — both as FA-facing context AND as a debugging
  signal for whoever audits the extraction.
  - For `high` confidence: ≤5 words. Terse anchors like "Distance
    printed at top.", "Addresses on route line.", "Date stamp visible."
  - For `medium` and `low`: ≤15 words. Explain the ambiguity or guess.
    Examples:
      medium: "Distance shown in km, converted to mi."
      low: "Date not on screenshot; guessed from context."
  Keep it ONE line per leaf.
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, an exact `quote` from the receipt, AND `token_ids: [N, N, ...]`
 . `token_ids` are integer indices from the numbered
  Document AI token list appended at the bottom of this prompt — pick
  the IDs of the tokens whose printed text covers your `quote`. If you
  can't identify them, omit `token_ids` and the post-pass will fall
  back to text matching; do NOT invent IDs.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`,
  `not_applicable_for_foreign`, or `not_applicable_for_transport`.
  Do NOT cite an unrelated quote with `document_span` to evidence
  a null value. `token_ids` MUST be omitted (or empty) for
  non-`document_span` evidence.
- `needs_review` is true for any value you guessed, and any field where
  you used `medium` or `low` confidence.
- `flags` stays empty unless you observe something irregular (e.g.
  unusually long distance for a single trip, multi-leg trip that may
  need to be split into separate lines).
"""


if __name__ == "__main__":
    sys.exit(run_extraction(parse_args(__doc__), PROMPT, RESPONSE_SCHEMA_PATH))
