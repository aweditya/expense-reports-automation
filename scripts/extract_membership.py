#!/usr/bin/env python3
"""Per-receipt Gemini extractor for membership-dues receipts.

Professional society / conference org membership receipts (ACM, IEEE,
USENIX, etc.) and similar annual / multi-year subscription receipts
that don't fit a more specific kind. The receipt schema has no
kind-specific detail block — `common` + `extras` carry everything
(date, amount, currency, org address as merchant_address, remarks).

Structured against `generated/response_schema_membership.json` so
the SDK enforces shape; the schema restricts `expense_type` to
`membership_dues`, which the foreign CSV emits as "Membership Dues -
Foreign" and the domestic CSV emits as "Membership Dues"
(csv_export::map_expense_type_*).

Dispatched by `local_app_simple.py` when the FA picks "Membership
Dues" in the upload-form file-row dropdown.

FA-policy note (carried in plan, not implemented yet): Stanford's
guidance is that membership dues should typically be paid on a
personal card (not Stanford P-card). Surfacing that distinction
needs a per-line payment_method schema field — out of scope here;
the extractor just captures the receipt as-is.
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_extraction


RESPONSE_SCHEMA_PATH = (
    Path(__file__).resolve().parent.parent / "generated" / "response_schema_membership.json"
)


PROMPT = """You are extracting a single membership-dues transaction line
from a receipt for a Stanford expense report. Typical examples: ACM,
IEEE, USENIX professional society annual dues; conference series
membership (e.g. ASPLOS Forever); journal subscriptions tied to a
trip's research.

Return a JSON array containing one transaction line object. Its shape
is enforced by the response schema — fill the values from the receipt.

# Reasoning rules

`expense_type`:
- ALWAYS `membership_dues` — the schema only accepts that value for
  membership receipts. Downstream the foreign CSV emits "Membership
  Dues - Foreign" and the domestic CSV emits "Membership Dues".

`country_of_activity`:
- Country of the organization charging the dues (from their address /
  context). For US-headquartered societies (ACM, IEEE), use "United
  States".

`original_currency` and `original_amount`:
- For USD receipts (printed amounts in dollars), set BOTH to null with
  `kind: system_generated, origin: not_applicable_for_domestic`.
- Only fill them when the receipt's printed amounts are in a non-USD
  currency (rare for membership dues — most large orgs bill USD even
  for international members).

`foreign_activity_type`:
- For domestic-org receipts: null with `kind: system_generated,
  origin: not_applicable_for_domestic`.
- For foreign-org receipts: pick conference / research_collaboration /
  fieldwork / other based on context. `other` is the typical default
  for membership-dues receipts (purpose isn't on the receipt).

`remarks`:
- One short sentence summarizing the membership — e.g. "ACM
  Professional Membership renewal", "USENIX 2026 annual dues",
  "IEEE Computer Society dues including digital library".

`extras.merchant_address`:
- The organization's billing address from the receipt header. Null
  with `kind: system_generated, origin: not_present_in_receipt` if
  absent.

`extras.printed_currency`:
- The currency literally on the receipt. ISO 4217 code if you see one;
  otherwise infer from a printed symbol.

# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when
  ambiguous, `high` when unambiguous on the receipt.
- `confidence_reason`: ≤5 words for `high`, ≤15 for medium/low.
- `evidence` for present values: `kind: document_span` with
  `filename`, `page`, exact `quote`, AND `token_ids: [N, ...]` from
  the numbered Document AI token list at the bottom of this prompt.
  Don't invent IDs — omit `token_ids` if uncertain and the post-pass
  falls back to text matching.
- `evidence` for null values: `kind: system_generated` with
  `origin: not_present_in_receipt` or `not_applicable_for_domestic`.
- `needs_review` is true for guessed values.
- `flags` stays empty unless something is irregular.
"""


if __name__ == "__main__":
    sys.exit(run_extraction(parse_args(__doc__), PROMPT, RESPONSE_SCHEMA_PATH))
