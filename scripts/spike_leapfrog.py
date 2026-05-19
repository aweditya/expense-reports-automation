#!/usr/bin/env python3
"""L.0 spike for the leapfrog architecture (docs/leapfrog-plan.md §6).

Prove the design works on real receipts BEFORE production stages land.
Specifically: does Gemini reliably return Document AI token_ids when
asked, can we trust those IDs (verifier pass rate), and does the
longer prompt regress extraction quality?

Approach (single spike — not production code):
  1. For each of 5 representative receipts:
     a. Call Document AI, get tokens with (id, text, bbox)
     b. Format as a numbered text list: `[p1.t142] FRANCISCO ...`
     c. Build a kind-agnostic Gemini prompt that asks for a flat list
        of (field_name, value, quote, token_ids) per receipt
     d. Call Gemini with image + token list + structured output
     e. For each returned field, run the verifier (Levenshtein-equiv
        via difflib.SequenceMatcher) comparing quote to the
        concatenated text of the claimed token_ids
     f. Look up bboxes from token_ids for sanity
  2. Aggregate per-receipt and write to
     .scratch/audit/leapfrog_spike.txt

Exit criteria (from plan §6):
  GO ahead with production stages:
    - >= 85% of entries return token_ids
    - >= 90% of returned token_ids pass the verifier
    - Token-list size < 30k chars on average
    - (extraction quality is a manual eyeball — values printed in
      the report; you compare to the receipts yourself)
  ABORT:
    - < 60% return token_ids (format is too hard for Gemini)
    - > 25% verifier rejection (hallucination rate too high)

Costs ~5 DocAI page-calls + 5 Gemini calls. User has unlimited
Gemini credits per project memory.
"""

import json
import pathlib
import sys
from difflib import SequenceMatcher

# Import the existing DocAI helper so we use the same processor + client
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from evidence_bbox import _ocr, _layout_text, _layout_bbox  # noqa: E402

from google import genai  # noqa: E402
from google.genai import types  # noqa: E402

REPO = pathlib.Path(__file__).resolve().parent.parent
OUT_TXT = REPO / ".scratch" / "audit" / "leapfrog_spike.txt"
OUT_JSON = REPO / ".scratch" / "audit" / "leapfrog_spike.json"
RECEIPTS_DIR = REPO / "receipts"

# Initial L.0 spike ran on 5 hardcoded receipts (see plan §6). The
# extended run globs all `receipts/*.{pdf,jpg,jpeg,png}` so we can
# measure how the architecture behaves at corpus scale before
# committing to L.1+. The flat spike schema still only asks for 6
# kind-agnostic fields per receipt (date, total_amount, currency,
# vendor_name, address, expense_type) — production wiring lands in
# L.3 with real kind-specific prompts.
SUPPORTED_EXTS = {".pdf", ".png", ".jpg", ".jpeg"}

# Project for Vertex/Gemini — same as the production extractors use.
PROJECT = "soe-agile-agents"
LOCATION = "global"  # matches DEFAULT_LOCATION in extractor_lib.py
MODEL = "gemini-3-flash-preview"  # matches DEFAULT_MODEL in extractor_lib.py

PROMPT_TEMPLATE = """You are extracting key fields from a receipt.

For each of these fields, return:
  - `name`: one of {{date, total_amount, currency, vendor_name, address, expense_type}}
  - `value`: the extracted value (a string; format dates as YYYY-MM-DD)
  - `quote`: a verbatim text snippet from the receipt that contains the value
  - `token_ids`: the integer ids of the tokens (from the numbered list below)
    that cover the quote text.

The token_ids MUST come from the provided numbered token list below.
Each token id is formatted as `t<N>` where N is a 1-based integer that
identifies the token uniquely across the WHOLE receipt (no page
distinction — page boundaries are invisible to you). Return token_ids
as integers (e.g. for `[t142]`, return `142`). Multi-page receipts use
one continuous ID range; just cite the tokens whose text covers your
quote.

If a field is not present on the receipt, omit that entry rather than
guessing.

# Numbered tokens from this receipt (from Document AI OCR):

{token_list}
"""

# Flat spike schema — does NOT use the production _meta-wrapped shape.
# Designed to be easy for Gemini to fill consistently.
RESPONSE_SCHEMA = {
    "type": "object",
    "properties": {
        "fields": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "value": {"type": "string"},
                    "quote": {"type": "string"},
                    "token_ids": {
                        "type": "array",
                        "items": {"type": "integer"},
                    },
                },
                "required": ["name", "value", "quote", "token_ids"],
            },
        },
    },
    "required": ["fields"],
}


def _norm(s: str) -> str:
    """Same normalization as evidence_bbox._norm but local copy so this
    spike doesn't depend on private helpers."""
    return " ".join(s.split()).lower()


def format_token_list(doc) -> tuple[str, dict[int, list[float]], dict[int, str]]:
    """Return (numbered-text-list, {id -> bbox}, {id -> text}).

    Format: `[t1] AIR\n[t2] INDIA\n[t3] PASSENGER\n...`

    Global IDs across the whole document — page boundaries are
    invisible in the prompt. This is the design we'd want in
    production anyway: one ID space per receipt, no page-
    disambiguation needed in the response_schema.
    """
    lines = []
    bbox_by_id: dict[int, list[float]] = {}
    text_by_id: dict[int, str] = {}
    global_idx = 0
    for page in doc.pages:
        for tok in page.tokens:
            text = _layout_text(doc, tok.layout).strip()
            bbox = _layout_bbox(tok.layout)
            if not text or bbox is None:
                continue
            global_idx += 1
            lines.append(f"[t{global_idx}] {text}")
            bbox_by_id[global_idx] = bbox
            text_by_id[global_idx] = text
    return "\n".join(lines), bbox_by_id, text_by_id


def call_gemini(client, image_bytes: bytes, mime: str, prompt: str) -> dict:
    """Send image + prompt to Gemini with structured output enforcement."""
    response = client.models.generate_content(
        model=MODEL,
        contents=[
            types.Part.from_bytes(data=image_bytes, mime_type=mime),
            prompt,
        ],
        config=types.GenerateContentConfig(
            response_mime_type="application/json",
            response_schema=RESPONSE_SCHEMA,
        ),
    )
    return json.loads(response.text)


def verifier_score(quote: str, claimed_text: str) -> float:
    """Levenshtein-equivalent via difflib. 1.0 = perfect match;
    0.0 = no overlap. Plan §5 suggests threshold ~0.7 (i.e. distance/
    len < 0.30); we report the raw ratio and let the report show the
    distribution."""
    if not quote or not claimed_text:
        return 0.0
    return SequenceMatcher(None, _norm(quote), _norm(claimed_text)).ratio()


def detect_mime(path: pathlib.Path) -> str:
    ext = path.suffix.lower()
    return {
        ".pdf": "application/pdf",
        ".png": "image/png",
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
    }.get(ext, "application/octet-stream")


def _union_bbox(rects: list[list[float]]) -> list[float]:
    return [
        min(r[0] for r in rects),
        min(r[1] for r in rects),
        max(r[2] for r in rects),
        max(r[3] for r in rects),
    ]


def run_one(label: str, receipt_path: pathlib.Path, client) -> dict:
    """Process one receipt; return per-receipt stats + raw fields +
    resolved bboxes (so the retrofit script can write them into the
    cached per-doc JSONs)."""
    print(f"  [{label}] {receipt_path.name}", flush=True)
    doc = _ocr(receipt_path)
    if doc is None:
        return {"label": label, "source_filename": receipt_path.name,
                "skipped": "unsupported mime"}
    token_list_text, bbox_by_id, text_by_id = format_token_list(doc)
    prompt = PROMPT_TEMPLATE.format(token_list=token_list_text)

    try:
        gemini_out = call_gemini(
            client,
            receipt_path.read_bytes(),
            detect_mime(receipt_path),
            prompt,
        )
    except Exception as err:
        return {"label": label, "source_filename": receipt_path.name,
                "error": str(err)}

    fields = gemini_out.get("fields", []) or []
    per_field = []
    n_returned_ids = 0
    n_pass_verifier = 0
    for f in fields:
        name = f.get("name", "")
        value = f.get("value", "")
        quote = f.get("quote", "")
        token_ids = f.get("token_ids", []) or []

        # Global token ID resolution — see format_token_list comment.
        claimed_text_parts = []
        valid_ids = []
        invalid_ids = []
        bboxes = []
        for tid in token_ids:
            if tid in text_by_id:
                claimed_text_parts.append(text_by_id[tid])
                valid_ids.append(tid)
                bboxes.append(bbox_by_id[tid])
            else:
                invalid_ids.append(tid)
        claimed_text = " ".join(claimed_text_parts)
        score = verifier_score(quote, claimed_text)
        # Threshold for "verifier passes" per plan §5
        passed = score >= 0.7

        if token_ids:
            n_returned_ids += 1
        if passed:
            n_pass_verifier += 1

        # Union of all valid token bboxes — one rect per field. Multi-
        # rect support (when a quote appears in multiple places) would
        # require Gemini to return multiple token_ids groups; spike
        # asks for one group, gets one union.
        resolved_bbox = _union_bbox(bboxes) if bboxes else None

        per_field.append({
            "name": name,
            "value": value,
            "quote": quote,
            "token_ids": token_ids,
            "claimed_text": claimed_text,
            "score": score,
            "verifier_passed": passed,
            "invalid_ids": invalid_ids,
            "bbox": resolved_bbox,
        })

    return {
        "label": label,
        "source_filename": receipt_path.name,
        "n_fields": len(fields),
        "n_returned_ids": n_returned_ids,
        "n_pass_verifier": n_pass_verifier,
        "prompt_chars": len(prompt),
        "token_list_chars": len(token_list_text),
        "fields": per_field,
    }


def write_report(results: list[dict]) -> None:
    OUT_TXT.parent.mkdir(parents=True, exist_ok=True)
    out: list[str] = []
    out.append("=" * 78)
    out.append("Leapfrog L.0 spike — token-id-grounded extraction")
    out.append("=" * 78)
    out.append("")
    # Per-receipt detail
    for r in results:
        out.append(f"## {r['label']}")
        if "skipped" in r:
            out.append(f"  SKIPPED: {r['skipped']}")
            out.append("")
            continue
        if "error" in r:
            out.append(f"  ERROR: {r['error']}")
            out.append("")
            continue
        out.append(f"  prompt_chars: {r['prompt_chars']:,}   token_list_chars: {r['token_list_chars']:,}")
        out.append(f"  fields returned: {r['n_fields']}   with token_ids: {r['n_returned_ids']}   verifier passed: {r['n_pass_verifier']}")
        out.append("")
        for f in r["fields"]:
            out.append(f"  - {f['name']}: {f['value']!r}")
            out.append(f"      quote:        {f['quote']!r}")
            out.append(f"      token_ids:    {f['token_ids']}")
            out.append(f"      claimed_text: {f['claimed_text']!r}")
            out.append(f"      verifier:     score={f['score']:.3f}  passed={f['verifier_passed']}")
            if f["invalid_ids"]:
                out.append(f"      INVALID IDS:  {f['invalid_ids']}")
            out.append("")

    # Aggregate totals
    total_fields = sum(r.get("n_fields", 0) for r in results if "fields" in r)
    total_returned = sum(r.get("n_returned_ids", 0) for r in results if "fields" in r)
    total_pass = sum(r.get("n_pass_verifier", 0) for r in results if "fields" in r)
    avg_token_chars = (
        sum(r.get("token_list_chars", 0) for r in results if "token_list_chars" in r)
        / max(1, sum(1 for r in results if "token_list_chars" in r))
    )

    pct_returned = 100 * total_returned / max(1, total_fields)
    pct_passed = 100 * total_pass / max(1, total_returned)

    out.append("=" * 78)
    out.append("TOTALS vs exit criteria (docs/leapfrog-plan.md §6)")
    out.append("=" * 78)
    out.append(f"  total fields:           {total_fields}")
    out.append(f"  returned token_ids:     {total_returned}  ({pct_returned:.1f}%)   ← target ≥85%, abort <60%")
    out.append(f"  verifier passed:        {total_pass}  ({pct_passed:.1f}% of returned)   ← target ≥90%, abort >25% reject")
    out.append(f"  avg token-list chars:   {avg_token_chars:,.0f}    ← target <30,000")
    out.append("")
    out.append("Manual eyeball needed: compare each receipt's `value` fields above")
    out.append("against the actual receipt PDFs/images to assess extraction-quality")
    out.append("regression. The spike does not auto-compare against the production")
    out.append("extractor's output (kinds differ).")

    OUT_TXT.write_text("\n".join(out) + "\n")
    print(f"wrote {OUT_TXT}")
    # Structured JSON for spike_leapfrog_retrofit.py to consume.
    OUT_JSON.write_text(json.dumps(results, indent=2) + "\n")
    print(f"wrote {OUT_JSON}")


def main() -> int:
    client = genai.Client(vertexai=True, project=PROJECT, location=LOCATION)
    # Glob the entire receipts/ directory — the L.0 extended run tests
    # at corpus scale rather than the original 5-receipt sample.
    receipts = sorted(
        p for p in RECEIPTS_DIR.iterdir()
        if p.suffix.lower() in SUPPORTED_EXTS
    )
    if not receipts:
        print(f"no receipts in {RECEIPTS_DIR}", file=sys.stderr)
        return 1
    print(f"running leapfrog spike against {len(receipts)} receipts...\n",
          flush=True)
    results = []
    for path in receipts:
        # Label = stem (filename without extension); makes it easy to
        # cross-reference with cached .scratch/spike/*.json in the
        # retrofit step.
        label = path.stem
        try:
            results.append(run_one(label, path, client))
        except Exception as err:
            print(f"  [{label}] ERROR: {err}", flush=True)
            results.append({"label": label, "source_filename": path.name,
                            "error": str(err)})
    write_report(results)
    return 0


if __name__ == "__main__":
    sys.exit(main())
