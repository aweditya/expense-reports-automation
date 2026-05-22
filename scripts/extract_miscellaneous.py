#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the miscellaneous expense kind.

Catch-all for receipts that don't fit a dedicated kind: poster
printing, software subscriptions, conference T-shirts, photocopying,
shipping fees, etc. The receipt schema has no kind-specific detail
block — `common` + `extras` carry everything (date, amount,
currency, vendor address, remarks).

Structured against `generated/response_schema_miscellaneous.json` so
the SDK enforces shape; the schema restricts `expense_type` to
`other_business_expense`, which the foreign CSV emits as
"Miscellaneous - Foreign" and the domestic CSV emits as
"Miscellaneous" (csv_export::map_expense_type_*).

Dispatched by `local_app_simple.py` when the FA picks the
"Miscellaneous" option in the upload-form file-row dropdown.
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_extraction


RESPONSE_SCHEMA_PATH = (
    Path(__file__).resolve().parent.parent / "generated" / "response_schema_miscellaneous.json"
)


PROMPT = """You are extracting a single transaction line from a
miscellaneous receipt for a Stanford expense report — typical examples:
poster printing, conference T-shirts, software subscription receipts,
photocopying, shipping. The receipt is attached as an image or PDF.

Return a JSON array containing one transaction line object. Its shape
is enforced by the response schema — fill the values from the receipt.

# Reasoning rules

`expense_type`:
- ALWAYS `other_business_expense` — the schema only accepts that one
  value for miscellaneous receipts. Maps to "Miscellaneous" on the
  domestic CSV and "Miscellaneous - Foreign" on the foreign CSV
  downstream.

`country_of_activity`:
- Country where the purchase happened (from merchant address or
  context). "United States" for US receipts.

`original_currency` and `original_amount`:
- For USD receipts (printed amounts in dollars), set BOTH to null with
  `kind: system_generated, origin: not_applicable_for_domestic`.
- Only fill them when the receipt's printed amounts are in a non-USD
  currency. `original_amount` is the amount as printed; `line_amount_usd`
  is the converted USD amount.

`foreign_activity_type`:
- For domestic receipts: null with `kind: system_generated,
  origin: not_applicable_for_domestic`.
- For foreign receipts: pick conference / research_collaboration /
  fieldwork / other based on context. `other` is the typical default
  for ancillary miscellaneous receipts.

`remarks`:
- One short sentence summarizing what the purchase was — e.g.
  "Poster printing for ASPLOS 2026", "Conference T-shirt at
  registration", "Software subscription receipt for trip-only use".

`extras.merchant_address`:
- The merchant's full address as printed on the receipt. Many small-
  shop receipts include the address in the header. Null with
  `kind: system_generated, origin: not_present_in_receipt` when
  absent.

`extras.printed_currency`:
- The currency literally on the receipt. ISO 4217 code if you see one
  (e.g. `USD`, `SGD`, `EUR`). Otherwise infer from a printed symbol
  (`$` alone is most likely USD; £ is GBP; € is EUR; ¥ is JPY/CNY).

# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous
  but defensible, `high` when unambiguous on the receipt. NOT a
  probability.
- `confidence_reason` is a short justification for the confidence level.
  REQUIRED for every leaf.
  - For `high`: ≤5 words.
  - For `medium` and `low`: ≤15 words explaining the ambiguity or guess.
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, an exact `quote` from the receipt, AND `token_ids: [N, N, ...]`.
  Use the numbered Document AI token list at the bottom of this prompt
  — pick the IDs whose printed text covers your `quote`. If you can't
  identify which tokens cover the quote, omit `token_ids` and the
  post-pass will fall back to text matching; do NOT invent IDs.
- `evidence` for null values: `kind: system_generated` with
  `origin: not_present_in_receipt` or `not_applicable_for_domestic`.
- `needs_review` is true for any value you guessed.
- `flags` stays empty unless something is irregular.
"""


if __name__ == "__main__":
    sys.exit(run_extraction(parse_args(__doc__), PROMPT, RESPONSE_SCHEMA_PATH))
