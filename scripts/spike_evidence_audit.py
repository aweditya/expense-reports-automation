#!/usr/bin/env python3
"""Phase 6 robustness-pass audit: characterize evidence + bbox coverage
across `.scratch/spike/*.json` so we can fix the right things.

For every leaf field (anything carrying a `_meta` block), classify the
state of its evidence:

  doc_span_with_bboxes   document_span evidence + non-empty bboxes
                         → workbench renders icon AND halo (best case)
  doc_span_no_bboxes     document_span evidence + has quote + EMPTY bboxes
                         → workbench renders icon but NO halo (the
                           "quote not found by DocAI matching" tail)
  doc_span_no_quote      document_span evidence but no quote string
                         → workbench renders icon, no halo possible
  non_doc_span           system_generated / user_input / document
                         → workbench renders no icon (correct when the
                           value isn't a source-document fact)
  no_evidence            evidence array empty
                         → workbench renders no icon

Emits:
  .scratch/audit/evidence_coverage.txt
    - top-line totals
    - per-field-path category breakdown
    - quote miss list (every doc_span_no_bboxes quote, grouped by
      field path) — these are the inputs to the neighborhood-search
      design

CLI:
  python scripts/spike_evidence_audit.py [path-to-jsons-dir]
  default jsons-dir is .scratch/spike/
"""

import json
import pathlib
import sys
from collections import defaultdict

REPO = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_DIR = REPO / ".scratch" / "spike"
OUT_DIR = REPO / ".scratch" / "audit"
OUT_FILE = OUT_DIR / "evidence_coverage.txt"


def classify(meta: dict) -> str:
    ev_list = meta.get("evidence") or []
    if not ev_list:
        return "no_evidence"
    # Look at the first evidence entry — pipeline convention is one
    # primary entry per field.
    ev = ev_list[0]
    if not isinstance(ev, dict):
        return "no_evidence"
    if ev.get("kind") != "document_span":
        return "non_doc_span"
    quote = ev.get("quote") or ""
    if not quote.strip():
        return "doc_span_no_quote"
    bboxes = ev.get("bboxes") or []
    if not bboxes:
        return "doc_span_no_bboxes"
    return "doc_span_with_bboxes"


def primary_quote(meta: dict) -> str:
    ev_list = meta.get("evidence") or []
    for ev in ev_list:
        if isinstance(ev, dict) and ev.get("kind") == "document_span":
            q = ev.get("quote")
            if q:
                return q
    return ""


def walk(obj, path: str, results: list) -> None:
    """Yield (path, category, quote) for every leaf with a `_meta` block."""
    if isinstance(obj, dict):
        meta = obj.get("_meta")
        if isinstance(meta, dict) and "evidence" in meta:
            results.append((path, classify(meta), primary_quote(meta)))
        for k, v in obj.items():
            if k == "_meta":
                continue
            walk(v, f"{path}.{k}" if path else k, results)
    elif isinstance(obj, list):
        for i, item in enumerate(obj):
            walk(item, f"{path}[{i}]", results)


def main() -> int:
    jsons_dir = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DIR
    targets = sorted(jsons_dir.glob("*.json"))
    if not targets:
        print(f"no JSONs in {jsons_dir}", file=sys.stderr)
        return 1

    OUT_DIR.mkdir(parents=True, exist_ok=True)

    totals = defaultdict(int)
    by_field = defaultdict(lambda: defaultdict(int))
    miss_quotes = defaultdict(list)  # field_path -> [quote, ...]
    receipt_summary = []  # one entry per JSON

    for path in targets:
        try:
            data = json.loads(path.read_text())
        except json.JSONDecodeError:
            continue
        record = data[0] if isinstance(data, list) else data
        if not isinstance(record, dict):
            continue
        source = record.get("source_filename") or path.stem
        results: list[tuple[str, str, str]] = []
        walk(record, "", results)

        rec_totals = defaultdict(int)
        for field_path, category, quote in results:
            totals[category] += 1
            by_field[field_path][category] += 1
            rec_totals[category] += 1
            if category == "doc_span_no_bboxes":
                miss_quotes[field_path].append(quote)

        receipt_summary.append((path.name, source, dict(rec_totals)))

    lines: list[str] = []
    lines.append("=" * 78)
    lines.append(f"Evidence + bbox audit  ({len(targets)} JSONs in {jsons_dir})")
    lines.append("=" * 78)
    lines.append("")
    lines.append("## Top-line totals")
    lines.append("")
    grand = sum(totals.values())
    for cat in [
        "doc_span_with_bboxes",
        "doc_span_no_bboxes",
        "doc_span_no_quote",
        "non_doc_span",
        "no_evidence",
    ]:
        n = totals.get(cat, 0)
        pct = (100.0 * n / grand) if grand else 0.0
        lines.append(f"  {cat:24s} {n:5d}  ({pct:5.1f}%)")
    lines.append(f"  {'TOTAL':24s} {grand:5d}")
    lines.append("")

    lines.append("## Per-receipt summary")
    lines.append("")
    lines.append(
        f"  {'json':45s} {'with_bbox':>10s} {'no_bbox':>8s} {'no_quote':>9s} {'non_ds':>7s} {'no_ev':>6s}"
    )
    for name, source, cats in receipt_summary:
        lines.append(
            f"  {name:45s} "
            f"{cats.get('doc_span_with_bboxes', 0):10d} "
            f"{cats.get('doc_span_no_bboxes', 0):8d} "
            f"{cats.get('doc_span_no_quote', 0):9d} "
            f"{cats.get('non_doc_span', 0):7d} "
            f"{cats.get('no_evidence', 0):6d}"
        )
    lines.append("")

    lines.append("## Per-field-path category breakdown")
    lines.append("(only paths with at least one occurrence)")
    lines.append("")
    lines.append(
        f"  {'field path':60s} {'WB':>4s} {'NB':>4s} {'NQ':>4s} {'ND':>4s} {'NE':>4s}"
    )
    lines.append(
        "  " + "-" * 60 + " " + "-" * 4 + " " + "-" * 4 + " " + "-" * 4 + " " + "-" * 4 + " " + "-" * 4
    )
    for field in sorted(by_field.keys()):
        cats = by_field[field]
        lines.append(
            f"  {field:60s} "
            f"{cats.get('doc_span_with_bboxes', 0):4d} "
            f"{cats.get('doc_span_no_bboxes', 0):4d} "
            f"{cats.get('doc_span_no_quote', 0):4d} "
            f"{cats.get('non_doc_span', 0):4d} "
            f"{cats.get('no_evidence', 0):4d}"
        )
    lines.append("")
    lines.append("KEY: WB=with_bbox  NB=no_bbox  NQ=no_quote  ND=non_doc_span  NE=no_evidence")
    lines.append("")

    lines.append("## Quote-miss list (doc_span quotes that produced NO bboxes)")
    lines.append("(every entry below = the neighborhood-search input data)")
    lines.append("")
    if not miss_quotes:
        lines.append("  (none — all document_span quotes matched at least one bbox)")
    else:
        for field in sorted(miss_quotes.keys()):
            quotes = miss_quotes[field]
            lines.append(f"  {field}  ({len(quotes)} miss(es))")
            seen = set()
            for q in quotes:
                norm = " ".join(q.split())
                if norm in seen:
                    continue
                seen.add(norm)
                lines.append(f"      {q!r}")
            lines.append("")

    OUT_FILE.write_text("\n".join(lines) + "\n")
    print(f"wrote {OUT_FILE} ({grand} leaf fields across {len(targets)} JSONs)")
    print("\n--- summary ---")
    for cat in [
        "doc_span_with_bboxes",
        "doc_span_no_bboxes",
        "doc_span_no_quote",
        "non_doc_span",
        "no_evidence",
    ]:
        n = totals.get(cat, 0)
        pct = (100.0 * n / grand) if grand else 0.0
        print(f"  {cat:24s} {n:5d}  ({pct:5.1f}%)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
