#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the ancillary_airline_fee kind.

Single-call: ancillary_details has 3 T3 leaves, well under Vertex's
schema-property ceiling, so we use the simple run_extraction helper
(like mileage / miscellaneous / membership) rather than the split-call
pattern.

Ancillary airline fees are the optional extras an airline charges on top
of the base fare: inflight Wi-Fi, checked / excess baggage, paid seat
selection or upgrade, priority boarding. The receipt is often the SAME
airline ticket / confirmation that carries the base fare — that base
fare belongs to a separate airfare line, so this extractor pulls ONLY
the ancillary fees and sums them into the line amount. The same PDF can
legitimately be uploaded as both `airfare` (base fare) and `ancillary`
(the fees); keeping them disjoint stops the trip total from
double-counting.

Dispatched by `local_app_simple.py` based on the FA's per-file kind
choice (`ancillary`) in the upload form.
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_extraction


RESPONSE_SCHEMA_PATH = (
    Path(__file__).resolve().parent.parent / "generated" / "response_schema_ancillary.json"
)


PROMPT = """You are extracting a single ancillary-airline-fee transaction
line from an airline receipt for a Stanford expense report. The receipt
is attached as an image or PDF — often a flight confirmation that lists
the base ticket fare AND optional extras (Wi-Fi, baggage, paid seats,
priority boarding).

Return a JSON array containing one transaction line object. Its shape is
enforced by the response schema — fill the values from the receipt.

# What counts as an ancillary fee

Inflight Wi-Fi, checked/excess baggage, paid seat selection or upgrade
(including "Preferred Zone Seat", "Paid Seat", "Economy Plus"), priority
boarding, and similar airline add-ons. The base ticket fare, taxes, and
carrier-imposed fees on the ticket itself are NOT ancillary — they
belong to the separate airfare line.

# Reasoning rules

`expense_type`:
- Always `ancillary_airline_fee`. The schema accepts no other value
  (the dispatcher routed the file here from the FA's kind choice).

`common.line_amount_usd` — THE CRITICAL FIELD:
- The SUM of the ancillary fees on the receipt ONLY. Do NOT include the
  base airfare, ticket taxes, or carrier-imposed fees on the ticket.
- Example: a ticket shows "New ticket $276.96", "Paid Seat (SFO-MIA)
  $10.26", "Paid Seat (MIA-SFO) $11.80". The ancillary line amount is
  $22.06 (the two seats), NOT $276.96 and NOT the $299.02 grand total.
- Example: "Airfare 259.48", "Wi-Fi Day Pass 8.00", "Preferred Zone Seat
  44.99". The ancillary line amount is $52.99 (wifi + seat), NOT 259.48.
- If the receipt shows no ancillary fee at all, set the value to null
  with `kind: system_generated, origin: not_present_in_receipt` and
  flag it — the FA likely chose the wrong kind for this receipt.

`ancillary_details.fee_category`:
- `wifi` for inflight internet / Wi-Fi day passes.
- `baggage` for checked / excess / overweight bag fees.
- `seat` for any paid seat: selection, upgrade, preferred/zone seat,
  Economy Plus.
- `priority_boarding` for priority/early boarding add-ons.
- `other` when the fee doesn't fit the above OR when the receipt bundles
  MORE THAN ONE category (e.g. Wi-Fi + a paid seat). When you use
  `other` for a mix, name the components in `description`.

`ancillary_details.airline`:
- The carrier that charged the fee, as printed (e.g. "United",
  "American", "Delta").

`ancillary_details.description`:
- A short FA-facing description of the ancillary fee(s) as printed. When
  several fees are present, list them with amounts, e.g. "Wi-Fi Day Pass
  $8.00 + Preferred Zone Seat $44.99" or "2 paid seats SFO-MIA $10.26 +
  MIA-SFO $11.80". This is the line-item text the FA reviews.

`common.date`:
- The purchase/charge date of the ancillary fee(s), ISO 8601
  (YYYY-MM-DD). Airline confirmations print this as "Date of purchase"
  or "Issued". If the fee was bought separately from the ticket (a later
  "Additional Purchase Summary"), prefer that purchase date.

`country_of_activity`:
- The country of the departure airport / where the flight operates.
  "United States" for US-domestic itineraries. Null with
  `kind: system_generated, origin: not_present_in_receipt` if the
  itinerary isn't determinable.

`original_currency` and `original_amount`:
- For USD receipts, set BOTH to null with `kind: system_generated,
  origin: not_applicable_for_domestic`.
- For non-USD receipts, fill them: `original_amount` is the ancillary
  fee total in the printed currency; `line_amount_usd` is the converted
  USD amount.

`foreign_activity_type`:
- For a USD/domestic fee: null with `kind: system_generated,
  origin: not_applicable_for_domestic`.
- For a foreign fee: pick from conference / research_collaboration /
  fieldwork / other based on context (usually `other`).

`remarks`:
- One short sentence: "{{fee description}} on {{airline}}
  ({{route if known}})." e.g. "Wi-Fi + preferred seat on United
  (SFO-MIA)."

`extras.merchant_address`:
- The airline's printed billing address if present; most e-ticket
  confirmations omit it, so null with `kind: system_generated,
  origin: not_present_in_receipt` is common.

`extras.printed_currency`:
- ISO 4217 if printed (e.g. `USD`), else inferred from a symbol with
  `medium` confidence.

# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous
  but defensible, `high` when unambiguous on the receipt. NOT a
  probability.
- `confidence_reason` is a short justification for the confidence level.
  REQUIRED for every leaf — both as FA-facing context AND as a debugging
  signal for whoever audits the extraction.
  - For `high` confidence: ≤5 words. "Seat fees itemized.", "Wi-Fi line
    printed.", "Airline in header."
  - For `medium` and `low`: ≤15 words. Explain the ambiguity. Examples:
      medium: "Summed two paid-seat lines into one amount."
      low: "Fee category unclear from abbreviated line."
  Keep it ONE line per leaf.
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, an exact `quote` from the receipt, AND `token_ids: [N, N, ...]`
 . `token_ids` are integer indices from the numbered Document AI token
  list appended at the bottom of this prompt — pick the IDs of the
  tokens whose printed text covers your `quote`. For a summed amount,
  cite the tokens of the fee lines you added. If you can't identify
  them, omit `token_ids` and the post-pass falls back to text matching;
  do NOT invent IDs.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`, or
  `not_applicable_for_foreign`. Do NOT cite an unrelated quote with
  `document_span` to evidence a null value. `token_ids` MUST be omitted
  (or empty) for non-`document_span` evidence.
- `needs_review` is true for any value you guessed, any field where you
  used `medium` or `low` confidence, and any summed amount.
- `flags` stays empty unless you observe something irregular (e.g. no
  ancillary fee found on the receipt, or a fee that looks like the base
  fare rather than an add-on).
"""


if __name__ == "__main__":
    sys.exit(run_extraction(parse_args(__doc__), PROMPT, RESPONSE_SCHEMA_PATH))
