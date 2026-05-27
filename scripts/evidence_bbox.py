#!/usr/bin/env python3
"""Populate `_meta.evidence[].bboxes` for each document_span evidence
entry in a per-doc extraction record, using Google Document AI OCR.

ONE Document AI call per document, then local token matching for each
quote. Idempotent: re-running on an already-populated record recomputes
against the same OCR output, so values stabilize.

PDFs and images both work — Document AI accepts either via mime_type.
For unsupported mime types the record is returned unchanged.

CLI (for ad-hoc debugging on one receipt):
    python scripts/evidence_bbox.py <per_doc_json> <receipt_path>

The CLI writes the populated record to stdout and does NOT mutate the
input file. Use scripts/retrofit_bboxes.py for in-place batch updates.
"""

import json
import pathlib
import re
import sys

from google.cloud import documentai

PROCESSOR = "projects/603261681824/locations/us/processors/4d07d363581d419d"
API_ENDPOINT = "us-documentai.googleapis.com"

# Document AI accepts a fixed set of mime types. Any extension not in
# this map skips the OCR step entirely (returns record unchanged).
MIME_BY_EXT = {
    ".pdf": "application/pdf",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".tif": "image/tiff",
    ".tiff": "image/tiff",
    ".gif": "image/gif",
    ".bmp": "image/bmp",
    ".webp": "image/webp",
}

_CLIENT = None


def _client():
    global _CLIENT
    if _CLIENT is None:
        _CLIENT = documentai.DocumentProcessorServiceClient(
            client_options={"api_endpoint": API_ENDPOINT}
        )
    return _CLIENT


def ocr_document(doc_path: pathlib.Path):
    """Single Document AI call. Returns documentai.Document, or None for
    unsupported mime types.

    Public since extractors call this directly so they
    can both inline the token list into Gemini's prompt AND pass the
    same Document into populate_bboxes for token_id resolution. Use
    instead of calling populate_bboxes' internal OCR (which does the
    same call but doesn't share state with the caller)."""
    mime = MIME_BY_EXT.get(doc_path.suffix.lower())
    if mime is None:
        return None
    raw = documentai.RawDocument(content=doc_path.read_bytes(), mime_type=mime)
    req = documentai.ProcessRequest(name=PROCESSOR, raw_document=raw)
    return _client().process_document(request=req).document


# Back-compat alias — `_ocr` was called from spike_leapfrog.py and the
# old in-file populate_bboxes. Keep the underscore name working while
# new code uses the public ocr_document.
_ocr = ocr_document


def _layout_text(doc, layout) -> str:
    parts = []
    for seg in layout.text_anchor.text_segments:
        s = int(seg.start_index) if seg.start_index else 0
        e = int(seg.end_index)
        parts.append(doc.text[s:e])
    return "".join(parts)


def _layout_bbox(layout):
    """Axis-aligned bbox [x0,y0,x1,y1] from a layout's normalized vertices,
    or None if the layout has no geometry."""
    verts = layout.bounding_poly.normalized_vertices
    if not verts:
        return None
    xs = [v.x for v in verts]
    ys = [v.y for v in verts]
    return [min(xs), min(ys), max(xs), max(ys)]


def format_tokens_for_prompt(doc) -> tuple[str, dict[int, list[float]], dict[int, str]]:
    """Return (numbered-text-list, {id -> bbox}, {id -> text}) for the
    whole document.

    Format: `[t1] AIR\n[t2] INDIA\n[t3] PASSENGER\n...`

    Global IDs across the entire document — page boundaries are
    invisible in the prompt. Matches spike_leapfrog.py's format
    (which validated 100% verifier pass across 417 fields)."""
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


def _verifier_score(quote: str, claimed_text: str) -> float:
    """Levenshtein-equivalent via stdlib difflib. 1.0 = perfect, 0.0 =
    no overlap. Threshold of 0.7 mirrors the spike + plan §5."""
    from difflib import SequenceMatcher
    if not quote or not claimed_text:
        return 0.0
    return SequenceMatcher(None, _norm(quote), _norm(claimed_text)).ratio()


# Currency symbols and ISO codes that Gemini's quote and DocAI's tokens
# disagree about all the time. Stripping them on BOTH sides means
# "$163.54" (one DocAI token) and "$ 163.54" (two DocAI tokens) and the
# quote "$163.54" all collapse to the same searchable "163.54".
_CURRENCY = re.compile(
    r"[\$₹€£¥]"  # common currency symbols
    r"|\b(?:USD|EUR|GBP|JPY|CNY|INR|SGD|AUD|CAD|HKD|CHF|NZD|SEK|NOK|DKK|MXN|ZAR|KRW|THB|MYR|IDR|PHP|VND|BRL|ARS|TWD|AED|SAR)\b"
)
# Punctuation that Gemini's quotes paraphrase but DocAI tokens split on
# (or vice versa). Preserve `.` (decimals like 163.54 — stripping would
# turn it into 163 54) and `@` (emails). Strip hyphens (dates like
# "20-JAN-24" vs "20 JAN 24"; hyphenated codes like "PIT-Pittsburgh"),
# slashes (dates like "03/05/2026"; names like "SRIRAM/ADITYA"), and
# percent (tip percentages like "20.00%" that DocAI sometimes splits
# off as its own token). Whitespace collapse handled separately.
_PUNCT = re.compile(r"[:;,()\[\]{}<>'\"!?\\|*+=&^~`\-/%]")


def _norm(s: str) -> str:
    """Whitespace-collapse + lowercase + strip currency markers + strip
    common punctuation. Applied identically on both sides (the quote
    being searched AND the token text being searched against) so any
    tokenization disagreement between Gemini and DocAI cancels out."""
    s = _CURRENCY.sub("", s)
    s = _PUNCT.sub(" ", s)  # replace with space, not empty — preserve word boundaries
    return " ".join(s.split()).lower()


def _find_quote_bboxes(doc, page_index: int, quote: str):
    """Find all bbox occurrences of `quote` on page `page_index` (1-based).
    Returns list of [x0,y0,x1,y1] rects, one per occurrence (each is the
    union of the tokens that cover the matched substring). Empty list
    when not found by any fallback.

    Fallback chain (mirrors workbench_spotcheck.js and the pymupdf spike):
      1. exact (normalized) substring match
      2. first 3 words (handles ellipsis-style quotes)
      3. longest distinctive word (>= 4 chars)
    """
    if not (1 <= page_index <= len(doc.pages)):
        return []
    page = doc.pages[page_index - 1]

    tokens = []
    for tok in page.tokens:
        text = _layout_text(doc, tok.layout)
        bbox = _layout_bbox(tok.layout)
        ntext = _norm(text)
        if ntext and bbox:
            tokens.append((ntext, bbox))
    if not tokens:
        return []

    # Concatenate normalized token text with single-space separators.
    # Track each token's [start, end) char span so we can map a char
    # offset (from page_text.find()) back to a token index.
    page_text_parts = [t[0] for t in tokens]
    page_text = " ".join(page_text_parts)
    token_spans = []  # (start_offset, end_offset) per token
    off = 0
    for text in page_text_parts:
        token_spans.append((off, off + len(text)))
        off += len(text) + 1  # +1 for the joining space

    def _char_to_token(c):
        # Linear scan; tokens-per-page is in the hundreds, fast enough.
        for i, (s, e) in enumerate(token_spans):
            if s <= c < e:
                return i
        return None

    def _union(rects):
        return [
            min(r[0] for r in rects),
            min(r[1] for r in rects),
            max(r[2] for r in rects),
            max(r[3] for r in rects),
        ]

    def _find_all(needle: str):
        out = []
        start = 0
        while True:
            idx = page_text.find(needle, start)
            if idx == -1:
                break
            first = _char_to_token(idx)
            last = _char_to_token(idx + len(needle) - 1)
            if first is not None and last is not None and first <= last:
                rects = [tokens[i][1] for i in range(first, last + 1)]
                out.append(_union(rects))
            start = idx + 1
        return out

    target = _norm(quote)
    if not target:
        return []

    rects = _find_all(target)
    if rects:
        return rects

    words = [w for w in target.replace("...", " ").split() if w]
    if len(words) > 3:
        rects = _find_all(" ".join(words[:3]))
        if rects:
            return rects

    distinctive = sorted([w for w in words if len(w) >= 4], key=len, reverse=True)
    if distinctive:
        rects = _find_all(distinctive[0])
        if rects:
            return rects

    return []


def populate_bboxes(record: dict, doc_path: pathlib.Path, doc=None) -> dict:
    """Walk record's `_meta.evidence[]` entries and populate `bboxes`
    for each `document_span` entry. Mutates record in place AND
    returns it (convenient for chaining).

    Two grounding paths ():
      1. `token_ids` present on the evidence entry (from a leapfrog-
         aware extractor): resolve via dict lookup against DocAI's
         tokens. Verifier compares the concatenated token text to
         the evidence `quote`; if it fails (Gemini hallucinated the
         ids), fall back to text matching for THAT entry.
      2. No `token_ids` (cached pre-leapfrog extractions; or any
         extractor still on the text-matching path): existing
         behavior — text-match the quote against DocAI tokens via
         _find_quote_bboxes.

    Both paths terminate with `bboxes` either set to a list or
    cleared. Idempotent: re-running stabilizes.

    `doc` parameter: callers that already ran DocAI (leapfrog
    extractors that need the token list for the Gemini prompt) can
    pass the Document in to avoid a second API call. Defaults to
    None, in which case we OCR fresh.

    Document AI errors are caught and logged; record is returned
    unchanged. Workbench falls back to "no halo" — same as
    pre-Stage-B behavior.
    """
    if doc is None:
        try:
            doc = ocr_document(doc_path)
        except Exception as err:
            print(
                f"warning: Document AI OCR failed for {doc_path}: {err}",
                file=sys.stderr,
            )
            return record
        if doc is None:
            return record

    # Pre-compute token text-by-id and bbox-by-id once. Same global-
    # ID scheme as format_tokens_for_prompt; required for token_id
    # resolution path.
    _, bbox_by_id, text_by_id = format_tokens_for_prompt(doc)

    def resolve_by_token_ids(token_ids: list[int], quote: str):
        """Return list-of-one [bbox] if the ids resolve AND the
        verifier passes; None otherwise (caller falls back to text
        matching)."""
        if not token_ids:
            return None
        rects = []
        claimed_texts = []
        for tid in token_ids:
            if tid in bbox_by_id:
                rects.append(bbox_by_id[tid])
                claimed_texts.append(text_by_id[tid])
        if not rects:
            return None
        score = _verifier_score(quote, " ".join(claimed_texts))
        if score < 0.7:  # plan §5 threshold
            return None
        # Union of all valid token bboxes — one rect per field.
        return [[
            min(r[0] for r in rects),
            min(r[1] for r in rects),
            max(r[2] for r in rects),
            max(r[3] for r in rects),
        ]]

    def walk(obj):
        if isinstance(obj, dict):
            meta = obj.get("_meta")
            if isinstance(meta, dict):
                for ev in meta.get("evidence") or []:
                    if not isinstance(ev, dict):
                        continue
                    if ev.get("kind") != "document_span":
                        continue
                    quote = ev.get("quote")
                    if not quote:
                        continue
                    # Path 1: token_ids if present + verifier passes
                    token_ids = ev.get("token_ids") or []
                    rects = resolve_by_token_ids(token_ids, quote)
                    # Path 2: fall back to text matching
                    if rects is None:
                        page = ev.get("page", 1)
                        rects = _find_quote_bboxes(doc, page, quote) or None
                    if rects:
                        ev["bboxes"] = rects
                    elif "bboxes" in ev:
                        # Both paths failed -> clear stale bboxes.
                        del ev["bboxes"]
            for v in obj.values():
                walk(v)
        elif isinstance(obj, list):
            for item in obj:
                walk(item)

    walk(record)
    return record


def _cli() -> int:
    if len(sys.argv) != 3:
        print(
            "usage: evidence_bbox.py <per_doc_json> <receipt_path>",
            file=sys.stderr,
        )
        return 2
    json_path = pathlib.Path(sys.argv[1])
    doc_path = pathlib.Path(sys.argv[2])
    data = json.loads(json_path.read_text())
    record = data[0] if isinstance(data, list) else data
    populate_bboxes(record, doc_path)
    json.dump(data, sys.stdout, indent=2)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(_cli())
