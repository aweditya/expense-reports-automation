#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the meal expense kind.

split into two parallel Gemini calls (was single-call until B1)
because needed `pre_tax_amount` + `tax_amount` in
`meal_details` for the precise 20% tip-cap validation, and adding
them to the single-call schema busted Vertex's property-count
ceiling (see docs/redesign-regrets.md 2026-05-22). Pattern mirrors
`extract_lodging.py`:

- "main" call:    extracts `common` + `meal_details` (the receipt-
                  level facts, now including the tax pair).
- "extras" call:  extracts the `extras` block (merchant_address,
                  printed_currency) alone.

Both calls see the **full receipt image**; they differ only in
their response schema and prompt focus. After both return, the
single-element output arrays are merged (the per-call top-level
keys are disjoint) and reduction sees the same per-doc shape a
single-call extraction would have produced.

Dispatched by `local_app_simple.py` based on the FA's per-file kind
choice in the upload form.
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_two_call_extraction


REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH_MAIN = REPO_ROOT / "generated" / "response_schema_meal_main.json"
SCHEMA_PATH_EXTRAS = REPO_ROOT / "generated" / "response_schema_meal_extras.json"


META_CONVENTION = """# _meta convention

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
  `page`, an exact `quote` from the receipt, AND `token_ids: [N, N, ...]`
 . `token_ids` are integer indices from the numbered
  Document AI token list appended at the bottom of this prompt — pick
  the IDs of the tokens whose printed text covers your `quote`. The
  concatenated text of those tokens should match (or closely paraphrase)
  the quote string. Use a continuous global ID range across all pages
  (page boundaries are invisible in the token list). When the quote
  spans multiple non-adjacent regions (e.g. "MUMBAI ... SAN FRANCISCO"),
  include the IDs of ALL the relevant tokens — they get unioned into one
  bounding box. If you genuinely can't identify which tokens cover the
  quote, omit `token_ids` and the post-pass will fall back to text
  matching; do NOT invent IDs.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`, or
  `not_applicable_for_foreign`. Do NOT cite an unrelated quote with
  `document_span` to evidence a null value. `token_ids` MUST be omitted
  (or empty) for non-`document_span` evidence.
- `needs_review` is true for any value you guessed, and any field where
  you used `medium` or `low` confidence.
- `flags` stays empty unless you observe something irregular.
"""


PROMPT_MAIN = f"""You are extracting a single meal transaction line from a receipt
for a Stanford expense report. The receipt is attached as an image.

Return a JSON array containing one transaction line object with
`common` and `meal_details` blocks. A parallel call to you (with a
different schema) is separately extracting the `extras` block —
focus only on what's in your schema here. Shape is enforced by the
response schema — fill the values from the receipt.

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

`meal_details.pre_tax_amount`:
- The subtotal BEFORE taxes and tip — usually labeled "Subtotal", "Food
  Subtotal", or just appears just above the tax line.
- If the receipt prints only the total and tax (no explicit subtotal),
  compute `total - tax - tip` and use `medium` confidence.
- If you genuinely can't find pre-tax (faded receipt, no subtotal printed,
  no math possible), set null with `kind: system_generated,
  origin: not_present_in_receipt`.

`meal_details.tax_amount`:
- The sales tax line — usually labeled "Tax", "Sales Tax", "GST", "VAT",
  "HST". Sum multiple tax lines (state + city) into one value.
- If no tax appears on the receipt at all (some take-out / coffee
  receipts), set null with `kind: system_generated,
  origin: not_present_in_receipt`. (Don't guess 0 — null is honest.)

`meal_details.tip_amount`:
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

`has_alcohol_on_receipt` and `alcohol_amount`:
- `has_alcohol_on_receipt` is true if ANY line item is alcohol (cocktail,
  beer, wine, etc.), even if the price is zero ("on the house").
- `alcohol_amount` is the SUM of all alcohol line item prices. Use 0.0 when
  alcohol is on the receipt but free; null when no alcohol is present.

{META_CONVENTION}"""


PROMPT_EXTRAS = f"""You are extracting auxiliary signals from a meal
receipt for a Stanford expense report. The receipt is attached as an
image.

A parallel call to you (with a different schema) is separately
extracting the receipt-level facts. Your job is the **`extras` block
only** — the merchant address and the printed currency. Return a JSON
array containing one transaction line object with only the `extras`
block populated. Shape is enforced by the response schema.

# Reasoning rules

`extras.merchant_address`:
- The full printed merchant address (street + city + region + country,
  if any are visible). This is the raw text; reduction parses it for
  the country signal.
- If only a city or partial address is printed, emit what's there.
- If the receipt prints no address at all, set null with
  `kind: system_generated, origin: not_present_in_receipt`.

`extras.printed_currency`:
- The currency literally on the receipt. ISO 4217 code if you see one
  (e.g. `USD`, `SGD`, `EUR`). Otherwise infer from a printed symbol
  (`$` alone is most likely USD; £ is GBP; € is EUR; ¥ is JPY/CNY —
  use medium confidence and let reduction disambiguate).

{META_CONVENTION}"""


if __name__ == "__main__":
    sys.exit(run_two_call_extraction(
        parse_args(__doc__),
        prompt_main=PROMPT_MAIN,
        prompt_extras=PROMPT_EXTRAS,
        schema_main=SCHEMA_PATH_MAIN,
        schema_extras=SCHEMA_PATH_EXTRAS,
        kind_label="meal",
    ))
