#!/usr/bin/env python3
"""Per-receipt Gemini extractor for the ground-transport expense kind.

B1: split into two parallel Gemini calls (was single-call until B1)
because Stage 9c needed `tip_amount` + `pre_tax_amount` + `tax_amount`
in `ground_transport_details` for the precise 20% tip-cap validation,
and adding them to the single-call schema busted Vertex's property-
count ceiling (see docs/redesign-regrets.md 2026-05-22). Pattern
mirrors `extract_lodging.py` and `extract_meal.py`:

- "main" call:    extracts `common` + `ground_transport_details`
                  (the receipt-level facts, now including the tax/tip
                  triple).
- "extras" call:  extracts the `extras` block (merchant_address —
                  always null for transport — and printed_currency)
                  alone.

After both calls return, the disjoint-keyed dicts are merged into the
same per-doc shape a single-call extraction would have produced.

Dispatched by `local_app_simple.py` based on the FA's per-file kind
choice in the upload form.
"""

from __future__ import annotations

import json
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from evidence_bbox import (
    format_tokens_for_prompt,
    ocr_document,
    populate_bboxes,
)
from extractor_lib import (
    GeminiCallFailed,
    detect_mime_type,
    parse_args,
    single_call,
)


REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH_MAIN = REPO_ROOT / "generated" / "response_schema_transport_main.json"
SCHEMA_PATH_EXTRAS = REPO_ROOT / "generated" / "response_schema_transport_extras.json"


META_CONVENTION = """# _meta convention

- `confidence` is ordinal: `low` when guessing, `medium` when ambiguous
  but defensible, `high` when unambiguous on the receipt. NOT a
  probability.
- `confidence_reason` is a short justification for the confidence level.
  REQUIRED for every leaf — both as FA-facing context AND as a debugging
  signal for whoever audits the extraction.
  - For `high` confidence: ≤5 words. Terse anchors like "Pickup address
    printed clearly.", "Total at top of receipt.", "USD literal on receipt."
  - For `medium` and `low`: ≤15 words. Explain the ambiguity or guess.
    Examples:
      medium: "USD inferred from `$` symbol; receipt doesn't say USD literally."
      low: "Field not visible on receipt; guessed from context."
  Keep it ONE line per leaf. The output budget is shared across thinking
  and tokens; verbose reasons crowd everything else out.
- `evidence` for present values: `kind: document_span` with `filename`,
  `page`, an exact `quote` from the receipt, AND `token_ids: [N, N, ...]`
  (Leapfrog L.4). `token_ids` are integer indices from the numbered
  Document AI token list appended at the bottom of this prompt — pick
  the IDs of the tokens whose printed text covers your `quote`. The
  concatenated text of those tokens should match (or closely paraphrase)
  the quote string. Use a continuous global ID range across all pages
  (page boundaries are invisible in the token list). When the quote
  spans multiple non-adjacent regions (e.g. pickup + drop-off addresses
  joined in one quote), include the IDs of ALL the relevant tokens —
  they get unioned into one bounding box. If you genuinely can't
  identify which tokens cover the quote, omit `token_ids` and the
  post-pass will fall back to text matching; do NOT invent IDs.
- `evidence` for null values: `kind: system_generated` with one of
  `origin: not_present_in_receipt`, `not_applicable_for_domestic`,
  `not_applicable_for_foreign`, or `not_applicable_for_transport`.
  Do NOT cite an unrelated quote with `document_span` to evidence
  a null value. `token_ids` MUST be omitted (or empty) for
  non-`document_span` evidence.
- `needs_review` is true for any value you guessed, and any field where
  you used `medium` or `low` confidence.
- `flags` stays empty unless you observe something irregular.
"""


PROMPT_MAIN = f"""You are extracting a single ground-transport
transaction line from a Lyft, Uber, or taxi receipt for a Stanford
expense report. The receipt is attached as an image (or multi-page
PDF — Uber receipts often have a second page with the pickup/dropoff
map).

Return a JSON array containing one transaction line object with
`common` and `ground_transport_details` blocks. A parallel call to
you (with a different schema) is separately extracting the `extras`
block — focus only on what's in your schema here. Shape is enforced
by the response schema; fill the values from the receipt.

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

`ground_transport_details.tip_amount`:
- The driver tip line — usually a distinct row labeled "Tip", "Driver
  Tip", or "Gratuity". On Uber/Lyft receipts this is shown right under
  the trip fare breakdown.
- If no tip line appears (e.g. some taxi receipts skip it), set null
  with `kind: system_generated, origin: not_present_in_receipt`.
  Don't guess 0 — null is honest.

`ground_transport_details.pre_tax_amount`:
- The fare base BEFORE taxes/fees/tip. On Uber/Lyft this is "Trip Fare"
  or appears as the subtotal above the fees block.
- If the receipt prints only a total, compute `total - taxes - tip` and
  use `medium` confidence. If you can't reconstruct it, set null.

`ground_transport_details.tax_amount`:
- The sum of taxes/fees (booking fees, sales tax, congestion charges,
  marketplace fees, etc.). Sum multiple lines into one value.
- If the receipt prints no tax/fee at all, set null with `kind:
  system_generated, origin: not_present_in_receipt`.

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

{META_CONVENTION}"""


PROMPT_EXTRAS = f"""You are extracting auxiliary signals from a
ground-transport receipt for a Stanford expense report. The receipt
is attached as an image (or multi-page PDF).

A parallel call to you (with a different schema) is separately
extracting the receipt-level facts. Your job is the **`extras` block
only** — the merchant address (always null for transport) and the
printed currency. Return a JSON array containing one transaction
line object with only the `extras` block populated. Shape is enforced
by the response schema.

# Reasoning rules

`extras.merchant_address`:
- Set value to null with `kind: system_generated,
  origin: not_applicable_for_transport`. Cab receipts don't have a
  merchant address in the same sense as a restaurant; the pickup and
  dropoff are captured in `ground_transport_details` in the parallel
  call.

`extras.printed_currency`:
- The currency literally on the receipt. ISO 4217 code if you see one
  (e.g. `USD`, `SGD`, `EUR`). Otherwise infer from a printed symbol
  (`$` alone is most likely USD; £ is GBP; € is EUR; ¥ is JPY/CNY —
  use medium confidence and let reduction disambiguate).

{META_CONVENTION}"""


def merge_transaction_lines(main_result: list, extras_result: list) -> list:
    """Merge the two single-call outputs into one transaction line.

    The two arrays' first elements have disjoint top-level keys —
    `main = {common, ground_transport_details}` and
    `extras = {extras}` — so a dict merge yields the same per-doc
    JSON shape a single-call extraction would have produced.
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

    client = genai.Client(
        vertexai=True, project=args.project, location=args.location
    )
    image_bytes = args.image.read_bytes()
    mime = detect_mime_type(args.image)

    doc = None
    try:
        doc = ocr_document(args.image)
    except Exception as err:
        print(
            f"warning: Document AI OCR failed for {args.image}: {err}; "
            "transport extraction will proceed without token-id grounding.",
            file=sys.stderr,
        )

    prompt_main = PROMPT_MAIN
    prompt_extras = PROMPT_EXTRAS
    if doc is not None:
        token_list_text, _, _ = format_tokens_for_prompt(doc)
        suffix = (
            "\n\n# Numbered Document AI tokens (for `token_ids` grounding)\n\n"
            + token_list_text
        )
        prompt_main = PROMPT_MAIN + suffix
        prompt_extras = PROMPT_EXTRAS + suffix

    shared_call_kwargs = dict(
        client=client,
        model=args.model,
        image_filename=args.image.name,
        image_bytes=image_bytes,
        mime=mime,
        output_base=args.output,
    )

    with ThreadPoolExecutor(max_workers=2) as executor:
        future_main = executor.submit(
            single_call,
            prompt=prompt_main,
            response_schema_path=SCHEMA_PATH_MAIN,
            diag_label="main",
            **shared_call_kwargs,
        )
        future_extras = executor.submit(
            single_call,
            prompt=prompt_extras,
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
    for entry in merged:
        if isinstance(entry, dict):
            entry["source_filename"] = args.image.name
            populate_bboxes(entry, args.image, doc=doc)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(merged, indent=2, ensure_ascii=False))
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
