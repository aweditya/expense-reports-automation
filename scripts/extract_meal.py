#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the meal expense kind.

One Gemini call per receipt image, structured against
`generated/response_schema_meal.json` so the SDK enforces shape. Other
expense kinds get their own extractor scripts (see `extract_transport.py`)
dispatched by `local_app_simple.py` based on the FA's per-file kind
choice in the upload form.

(Originally `spike_extract.py` from M4; renamed when the per-kind
multi-extractor architecture landed in Phase 2.)
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_extraction


RESPONSE_SCHEMA_PATH = (
    Path(__file__).resolve().parent.parent / "generated" / "response_schema_meal.json"
)


PROMPT = """You are extracting a single meal transaction line from a receipt
for a Stanford expense report. The receipt is attached as an image.

Return a JSON array containing one transaction line object. Its shape is
enforced by the response schema — fill the values from the receipt.

# Reasoning rules

`expense_type`:
- Use `business_meal` for a meal the payee took with one or more guests.
- Use `group_travel_meal` ONLY when the receipt clearly indicates multiple
  Stanford travelers — multiple guests/diners is NOT a group_travel signal.
- Alcohol presence is NOT encoded in `expense_type`. Use the dedicated
  `has_alcohol_on_receipt` boolean (and `alcohol_amount` for the dollar
  total). The workbench will assemble "Business Meal with Alcohol" or
  "Group Travel Meal with Alcohol" from those two fields when displaying.
- Sales tax labels like GST, VAT, HST, and "Sales Tax" are tax categories,
  NOT expense_type signals.

`tip_amount`:
- Look below the subtotal/tax block. Common labels: `Tip`, `Gratuity`,
  `Service Charge`. The tip can be handwritten in or printed.
- If `Total > Subtotal + Tax`, the difference is likely tip even when not
  explicitly labeled — use `medium` confidence in that case.
- If the receipt has space for a tip but you can't read the value, use
  `value: null` with `low` confidence and `needs_review: true`.

`original_currency` and `original_amount`:
- For USD receipts (i.e., the printed amounts are in dollars), set BOTH to
  null with `kind: system_generated, origin: not_applicable_for_domestic`.
- Only fill them when the receipt's printed amounts are in a non-USD
  currency. `original_amount` is the amount as printed on the receipt in
  that currency; `line_amount_usd` is the converted USD amount.

`country_of_activity`:
- Fill from the merchant address when present (city/state/country gives the
  country). Pass 2 will null this for domestic expenses if needed.

`extras.merchant_address`:
- The full printed merchant address (street + city + region + country, if
  any are visible). This is the raw text; reduction parses it later.

`extras.printed_currency`:
- The currency literally on the receipt. ISO 4217 code if you see one
  (e.g. `USD`, `SGD`, `EUR`). Otherwise infer from a printed symbol
  (`$` alone is most likely USD; £ is GBP; € is EUR; ¥ is JPY/CNY —
  use medium confidence and let reduction disambiguate).

`has_alcohol_on_receipt` and `alcohol_amount`:
- `has_alcohol_on_receipt` is true if ANY line item is alcohol (cocktail,
  beer, wine, etc.), even if the price is zero ("on the house").
- `alcohol_amount` is the SUM of all alcohol line item prices. Use 0.0 when
  alcohol is on the receipt but free; null when no alcohol is present.

# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous but
  defensible, `high` when unambiguous on the receipt. NOT a probability.
- `confidence_reason` is a short justification for the confidence level.
  REQUIRED for every leaf — both as FA-facing context AND as a debugging
  signal for whoever audits the extraction.
  - For `high` confidence: ≤5 words. Terse anchors like "Total clearly
    printed.", "Date in receipt header.", "USD literal on receipt."
  - For `medium` and `low`: ≤15 words. Explain the ambiguity or guess.
    Examples:
      medium: "USD inferred from `$` symbol; receipt doesn't say USD literally."
      medium: "Tip handwritten in pen, slightly hard to read but legible."
      low: "Field not visible on receipt; guessed from context."
  Keep it ONE line per leaf. The output budget is shared across thinking
  and tokens; verbose reasons crowd everything else out.
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, and an exact `quote` from the receipt.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`, or
  `not_applicable_for_foreign`. Do NOT cite an unrelated quote with
  `document_span` to evidence a null value.
- `needs_review` is true for any value you guessed, and any field where
  you used `medium` or `low` confidence.
- `flags` stays empty unless you observe something irregular.
"""


if __name__ == "__main__":
    sys.exit(run_extraction(parse_args(__doc__), PROMPT, RESPONSE_SCHEMA_PATH))
