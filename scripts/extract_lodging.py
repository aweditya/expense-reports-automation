#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the lodging expense kind.

One Gemini call per hotel folio. Structured against
`generated/response_schema_lodging.json` so the SDK enforces shape;
the schema restricts `expense_type` to the two `lodging_*` variants.

Phase 3 v1 focuses on English-language folios — clean PDFs from US
hotel chains (Hilton/Hampton/Hyatt/Sheraton/Homewood/Hyatt Place)
that are the bulk of the corpus the FA submits. Multilingual support
(French/German/Japanese folios in the corpus) is a v2 follow-up.

Dispatched by `local_app_simple.py` based on the FA's per-file kind
choice in the upload form.
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_extraction


RESPONSE_SCHEMA_PATH = (
    Path(__file__).resolve().parent.parent / "generated" / "response_schema_lodging.json"
)


PROMPT = """You are extracting a single lodging transaction line from a
hotel folio for a Stanford expense report. The folio is attached as an
image or PDF (folios are typically multi-page; later pages often itemize
nightly charges).

Return a JSON array containing one transaction line object. Its shape
is enforced by the response schema — fill the values from the folio.

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
- One short sentence: "{nights}-night stay at {hotel} in {city}
  from {check_in} to {check_out}."

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
  matches `check_in_date`. The last entry is the night BEFORE
  `check_out_date` (you don't pay for the checkout day's night).
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
  and `taxes_and_fees = 0.0`, and use `medium` confidence on the
  whole `nightly_rates` array.
- Carry a single confidence on the whole array (not per-night):
  `high` if every line is clearly itemized, `medium` if you had to
  back-compute from a total, `low` if the breakdown is unclear.

# _meta convention

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


if __name__ == "__main__":
    sys.exit(run_extraction(parse_args(__doc__), PROMPT, RESPONSE_SCHEMA_PATH))
