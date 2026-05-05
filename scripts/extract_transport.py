#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the ground-transport expense kind.

One Gemini call per receipt image (Lyft Ride Report, Uber receipt,
or taxi receipt — single-page or multi-page PDFs both work).
Structured against `generated/response_schema_transport.json` so the
SDK enforces shape; the schema also restricts `expense_type` to only
the two `ground_transportation_*` variants.

Dispatched by `local_app_simple.py` based on the FA's per-file kind
choice in the upload form.
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_extraction


RESPONSE_SCHEMA_PATH = (
    Path(__file__).resolve().parent.parent / "generated" / "response_schema_transport.json"
)


PROMPT = """You are extracting a single ground-transport transaction line
from a Lyft, Uber, or taxi receipt for a Stanford expense report. The
receipt is attached as an image (or multi-page PDF — Uber receipts often
have a second page with the pickup/dropoff map).

Return a JSON array containing one transaction line object. Its shape
is enforced by the response schema — fill the values from the receipt.

# Reasoning rules

`expense_type`:
- `ground_transportation_domestic` when both pickup and dropoff are in
  the United States.
- `ground_transportation_foreign` when at least one is outside the US.
- The schema only accepts these two values for transport receipts. Do
  NOT pick airfare or any other category.

`ground_transport_details.service_provider`:
- The ride-share or taxi company name as it appears on the receipt
  (e.g. "Uber", "Lyft", "Yellow Cab", "Curb"). Use the literal brand
  name from the receipt header / logo, not a generic term like "taxi".

`ground_transport_details.origin`:
- The pickup address. Full street address if visible. On Lyft "Ride
  report" PDFs this appears alongside the route map (the upper of the
  two addresses). On Uber receipts the addresses are on a second page.

`ground_transport_details.destination`:
- The dropoff address. Same source as origin.

`country_of_activity`:
- The country of the dropoff address. "United States" for US rides.

`original_currency` and `original_amount`:
- For USD receipts (printed amounts in dollars), set BOTH to null with
  `kind: system_generated, origin: not_applicable_for_domestic`.
- Only fill them when the receipt's printed amounts are in a non-USD
  currency. `original_amount` is the amount as printed; `line_amount_usd`
  is the converted USD amount.

`foreign_activity_type`:
- For domestic rides: null with `kind: system_generated,
  origin: not_applicable_for_domestic`.
- For foreign rides: pick from conference / research_collaboration /
  fieldwork / other based on context (often `other` since cab purpose
  is rarely on the receipt itself).

`remarks`:
- One short sentence summarizing the ride: e.g. "Lyft from Palo Alto
  to Stanford on April 12" or "UberX from Stanford to San Francisco
  on May 1, 35 miles".

`extras.merchant_address`:
- Set value to null with `kind: system_generated,
  origin: not_applicable_for_transport`. Cab receipts don't have a
  merchant address in the same sense as a restaurant; the pickup and
  dropoff are captured in `ground_transport_details` instead.

`extras.printed_currency`:
- The currency literally on the receipt. ISO 4217 code if you see one
  (e.g. `USD`, `SGD`, `EUR`). Otherwise infer from a printed symbol
  (`$` alone is most likely USD; £ is GBP; € is EUR; ¥ is JPY/CNY —
  use medium confidence and let reduction disambiguate).

# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous
  but defensible, `high` when unambiguous on the receipt. NOT a
  probability.
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, and an exact `quote` from the receipt.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`,
  `not_applicable_for_foreign`, or `not_applicable_for_transport`.
  Do NOT cite an unrelated quote with `document_span` to evidence
  a null value.
- `needs_review` is true for any value you guessed, and any field where
  you used `medium` or `low` confidence.
- `flags` stays empty unless you observe something irregular.
"""


if __name__ == "__main__":
    sys.exit(run_extraction(parse_args(__doc__), PROMPT, RESPONSE_SCHEMA_PATH))
