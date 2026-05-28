#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the conference-registration kind.

Two parallel Gemini calls per registration receipt:

- "main" call:    extracts `common` + `conference_registration_details`
                  (conference name, order number, ticket type, attendee
                  name, registration system).
- "extras" call:  extracts the `extras` block — merchant address and
                  printed currency (reduction reads these for the
                  foreign-vs-domestic and FX signals).

Both calls receive the full receipt; only the response schema differs.
The split is forced by Vertex's Schema validator rejecting the
single-call schema (common + 5 detail leaves + extras) with a 400 —
same property-count ceiling that splits lodging. Results merge into the
`{common, conference_registration_details, extras}` shape a single call
would have produced; reduction sees no difference.

Conference dates (conference_start_date / conference_end_date) and
meals_included are NOT extracted here: the first two are reduction-
derived from supporting conference docs (T1 fallback when none), and
meals_included is FA-entered (T1).

Dispatched by `local_app_simple.py` based on the per-file kind choice
in the upload form.
"""

from __future__ import annotations

import sys
from pathlib import Path

from extractor_lib import parse_args, run_two_call_extraction


REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH_MAIN = (
    REPO_ROOT / "generated" / "response_schema_conference_registration_main.json"
)
SCHEMA_PATH_EXTRAS = (
    REPO_ROOT / "generated" / "response_schema_conference_registration_extras.json"
)


# Identical _meta convention across both calls. Defined once so the two
# prompts can't drift out of sync.
META_CONVENTION = """# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous
  but defensible, `high` when unambiguous on the receipt. NOT a
  probability.
- `confidence_reason` is a short justification for the confidence
  level. REQUIRED for every leaf — both as FA-facing context AND as
  a debugging signal for whoever audits the extraction.
  - For `high` confidence: ≤5 words. "Conference name in header.",
    "Order ID printed clearly.", "Attendee in 'Registered to' field."
  - For `medium` and `low`: ≤15 words. Explain the ambiguity.
    Examples:
      medium: "Ticket tier inferred from line-item label, not a named field."
      low: "Registration system guessed from page styling; no logo printed."
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, an exact `quote` from the receipt, AND `token_ids: [N, N, ...]`
 . `token_ids` are integer indices from the numbered Document AI token
  list appended at the bottom of this prompt — pick the IDs of the
  tokens whose printed text covers your `quote`. The concatenated text
  of those tokens should match (or closely paraphrase) the quote string.
  Use a continuous global ID range across all pages (page boundaries are
  invisible in the token list). When the quote spans multiple
  non-adjacent regions, include the IDs of ALL the relevant tokens —
  they get unioned into one bounding box. If you genuinely can't
  identify which tokens cover the quote, omit `token_ids` and the
  post-pass will fall back to text matching; do NOT invent IDs.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`,
  or `not_applicable_for_foreign`. Do NOT cite an unrelated quote with
  `document_span` to evidence a null value. `token_ids` MUST be omitted
  (or empty) for non-`document_span` evidence.
- `needs_review` is true for any value you guessed, and any field
  where you used `medium` or `low` confidence.
- `flags` stays empty unless you observe something irregular
  (e.g. registration partially covered by a grant/sponsor, a refunded
  or cancelled line, a bundled hotel/meal charge mixed into the total).
"""


PROMPT_MAIN = f"""You are extracting receipt-level facts from a
conference-registration receipt for a Stanford expense report. The
receipt is attached as an image or PDF (registration confirmations come
from systems like Whova, Cvent, ACM RegOnline, Eventbrite, or USENIX,
and usually print the conference name, an order/registration ID, the
attendee, and the ticket tier that was purchased).

Return a JSON array containing one transaction line object with
`common` and `conference_registration_details` blocks. A parallel call
to you (with a different schema) is separately extracting the `extras`
block — focus only on what's in your schema here. Shape is enforced by
the response schema; fill the values from the receipt.

# Reasoning rules

`expense_type`:
- Always `conference_registration` — the schema accepts no other value
  for this kind. There is no domestic/foreign variant; foreignness is
  carried by `original_currency` and `country_of_activity` below.

`conference_registration_details.conference_name`:
- The conference name as printed (e.g. "ASPLOS 2026", "ACM SIGCOMM
  2025", "NeurIPS 2025"). Prefer the canonical short name plus year
  when both are printed; otherwise the full title as it appears. Do
  not include the registration vendor's name (Whova/Cvent/etc.).

`conference_registration_details.order_number`:
- The order confirmation number, registration ID, or invoice number
  the receipt prints to identify this purchase. If several IDs are
  printed, prefer the one labeled order/confirmation/registration.

`conference_registration_details.ticket_type`:
- WHAT was purchased — the registration tier/package as printed. This
  is the FA-facing line-item description. Examples as they commonly
  appear: "Main Conference Only", "Conference + Workshops", "Workshops/
  Tutorials Only", "Full Pass", "Virtual Attendance Only", "Student
  Registration". Quote the receipt's own wording; combine components
  when the receipt bundles them (e.g. "Conference + Banquet").

`conference_registration_details.attendee_name`:
- The registrant / attendee name on the receipt (the person attending,
  not necessarily the cardholder).

`conference_registration_details.registration_system`:
- `whova`, `cvent`, `acm_regonline`, `eventbrite`, or `usenix` when the
  receipt's branding/format clearly indicates it (logo, footer, sender
  domain, portal styling). Default `other` when unrecognized — do not
  guess a specific vendor at high confidence without a clear signal.

`date`:
- The order/registration/charge date as printed on the receipt, in ISO
  8601 (YYYY-MM-DD). This is the purchase date, NOT the conference's
  scheduled dates (those are derived elsewhere).

`country_of_activity`:
- The country where the conference is held, if determinable from the
  receipt (venue/city printed on the confirmation). "United States" for
  US conferences. Null with `kind: system_generated, origin:
  not_present_in_receipt` if the receipt does not say where the
  conference takes place.

`original_currency` and `original_amount`:
- For USD receipts (amounts printed in dollars), set BOTH to null with
  `kind: system_generated, origin: not_applicable_for_domestic`.
- For non-USD receipts (EUR, GBP, JPY, etc.), fill them.
  `original_amount` is the registration total as printed in that
  currency; `line_amount_usd` is the converted USD amount.

`foreign_activity_type`:
- For a USD/domestic registration: null with `kind: system_generated,
  origin: not_applicable_for_domestic`.
- For a foreign registration: `conference`.

`remarks`:
- One short sentence: "{{ticket_type}} registration for
  {{conference_name}}; attendee {{attendee_name}}."

{META_CONVENTION}"""


PROMPT_EXTRAS = f"""You are extracting auxiliary signals from a
conference-registration receipt for a Stanford expense report. The
receipt is attached as an image or PDF.

A parallel call to you (with a different schema) is separately
extracting the receipt-level facts. Your job is the **`extras` block
only** — the merchant address and the printed currency. Return a JSON
array containing one transaction line object with only the `extras`
block populated. Shape is enforced by the response schema.

# Reasoning rules

`extras.merchant_address`:
- The full printed billing/vendor address on the receipt (the
  conference organizer or the registration vendor — street + city +
  region + country, if any are visible). This is the raw text;
  reduction parses it for the country signal.
- If only a city or partial address is printed, emit what's there.
- If the receipt prints no address at all, set null with
  `kind: system_generated, origin: not_present_in_receipt`.

`extras.printed_currency`:
- The currency literally on the receipt. ISO 4217 code if you see one
  (e.g. `USD`, `EUR`, `GBP`). Otherwise infer from a printed symbol
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
        kind_label="conference_registration",
    ))
