#!/usr/bin/env python3
"""Phase 6 Stage B spike: verify that pymupdf.search_for() can locate
extracted-evidence quotes inside the real receipt PDFs.

This is a throwaway de-risking script. The test is its stdout on real
inputs, not unit tests. If results look good, the same quote-search
capability is also available client-side via PDF.js getTextContent
(which is what production would actually use). If results look bad,
the whole "quote -> bbox" approach is wrong and we rethink.

Reads cached per-doc extraction JSONs from .scratch/spike/, pulls every
document_span evidence entry, opens the referenced PDF page, and runs
page.search_for(quote). Reports per-quote: match count, rects, page
dimensions, and one of the categorical outcomes from the plan:
  HIT  - exactly one rect (great)
  MULT - multiple rects (ambiguity, same as current text-search)
  MISS - zero rects (quote doesn't appear verbatim - fallback needed)
  ERR  - couldn't open / no text layer (scanned image)
"""

import json
import pathlib
import sys

import pymupdf

REPO = pathlib.Path(__file__).resolve().parent.parent
SPIKE_DIR = REPO / ".scratch" / "spike"
RECEIPTS_DIR = REPO / "receipts"


def collect_evidence(rec: dict) -> list[tuple[str, dict]]:
    """Walk the per-doc record and yield (field_path, evidence) pairs."""
    out: list[tuple[str, dict]] = []

    def walk(obj, path: str) -> None:
        if isinstance(obj, dict):
            for k, v in obj.items():
                if k == "_meta" and isinstance(v, dict):
                    ev_list = v.get("evidence") or []
                    for ev in ev_list:
                        if isinstance(ev, dict) and ev.get("kind") == "document_span":
                            out.append((path, ev))
                else:
                    walk(v, f"{path}.{k}" if path else k)
        elif isinstance(obj, list):
            for i, item in enumerate(obj):
                walk(item, f"{path}[{i}]")

    walk(rec, "")
    return out


def test_one_json(json_path: pathlib.Path) -> dict:
    with json_path.open() as f:
        records = json.load(f)
    rec = records[0]
    source = rec.get("source_filename") or json_path.stem
    pdf_path = RECEIPTS_DIR / source

    print(f"\n{'=' * 72}")
    print(f"{json_path.name}  ->  {source}")
    print(f"{'=' * 72}")

    if not pdf_path.exists():
        print(f"  SKIP - PDF not found at {pdf_path}")
        return {"hit": 0, "mult": 0, "miss": 0, "err": 0, "skip": 1}

    if pdf_path.suffix.lower() != ".pdf":
        print(f"  SKIP - not a PDF (got {pdf_path.suffix})")
        return {"hit": 0, "mult": 0, "miss": 0, "err": 0, "skip": 1}

    try:
        doc = pymupdf.open(pdf_path)
    except Exception as e:
        print(f"  ERR - could not open PDF: {e}")
        return {"hit": 0, "mult": 0, "miss": 0, "err": 1, "skip": 0}

    counts = {"hit": 0, "mult": 0, "miss": 0, "err": 0, "skip": 0}
    evidence = collect_evidence(rec)
    print(f"  {len(evidence)} document_span evidence entries\n")

    for field_path, ev in evidence:
        page_num = ev.get("page", 1)
        quote = ev.get("quote", "")
        if not quote:
            continue

        try:
            page = doc[page_num - 1]
        except IndexError:
            print(f"  ERR  {field_path}  page {page_num} out of range")
            counts["err"] += 1
            continue

        w, h = page.rect.width, page.rect.height

        # Try the literal quote first.
        rects = page.search_for(quote)
        used = quote

        # If literal failed and quote has ellipses, try first 3 words.
        if not rects and "..." in quote:
            words = quote.replace("...", " ").split()
            if len(words) >= 3:
                prefix = " ".join(words[:3])
                rects = page.search_for(prefix)
                used = f"{prefix!r} (3-word prefix; original had '...')"

        # If still nothing, try the longest single word >= 4 chars.
        if not rects:
            words = [w for w in quote.replace("...", " ").split() if len(w) >= 4]
            if words:
                distinctive = max(words, key=len)
                rects = page.search_for(distinctive)
                used = f"{distinctive!r} (distinctive word fallback)"

        n = len(rects)
        if n == 0:
            tag = "MISS"
            counts["miss"] += 1
        elif n == 1:
            tag = "HIT "
            counts["hit"] += 1
        else:
            tag = "MULT"
            counts["mult"] += 1

        print(f"  {tag}  {field_path}  p{page_num} ({w:.0f}x{h:.0f})")
        print(f"        quote: {quote!r}")
        if used != quote:
            print(f"        used:  {used}")
        for r in rects[:3]:
            print(f"        rect:  ({r.x0:.1f}, {r.y0:.1f}) -> ({r.x1:.1f}, {r.y1:.1f})")
        if n > 3:
            print(f"        ... and {n - 3} more")

    doc.close()
    return counts


def main() -> int:
    # Test all cached spike JSONs we have. If a particular one is desired,
    # pass its stem as an arg.
    targets = sorted(SPIKE_DIR.glob("*.json"))
    if len(sys.argv) > 1:
        wanted = set(sys.argv[1:])
        targets = [p for p in targets if p.stem in wanted or p.name in wanted]

    if not targets:
        print("no cached spike JSONs found in .scratch/spike/")
        return 1

    totals = {"hit": 0, "mult": 0, "miss": 0, "err": 0, "skip": 0}
    for p in targets:
        c = test_one_json(p)
        for k in totals:
            totals[k] += c[k]

    print(f"\n{'=' * 72}")
    print(f"TOTALS across {len(targets)} JSONs:")
    print(f"  HIT  (1 rect)   : {totals['hit']}")
    print(f"  MULT (>1 rects) : {totals['mult']}")
    print(f"  MISS (0 rects)  : {totals['miss']}")
    print(f"  ERR             : {totals['err']}")
    print(f"  SKIP            : {totals['skip']}")
    grand = sum(v for k, v in totals.items() if k != "skip")
    if grand:
        hit_rate = 100 * totals["hit"] / grand
        any_rate = 100 * (totals["hit"] + totals["mult"]) / grand
        print(f"  exact-hit rate  : {hit_rate:.1f}%  ({totals['hit']}/{grand})")
        print(f"  any-match rate  : {any_rate:.1f}%  ({totals['hit'] + totals['mult']}/{grand})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
