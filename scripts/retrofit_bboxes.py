#!/usr/bin/env python3
"""One-shot: iterate .scratch/spike/*.json, populate bboxes via Document
AI, write back. Used for local validation of the workbench halo without
re-running Gemini (~3 min, costs API quota).

The cached spike JSONs were extracted before bboxes existed. This script
adds bboxes to every document_span evidence entry so the workbench will
render halos as if Stage B.3 had run the extractors fresh.

Costs one Document AI page-call per receipt. ~20 receipts in the corpus
at present, so cents total.

Optional positional args filter targets by filename or stem:
    python scripts/retrofit_bboxes.py                       # all JSONs
    python scripts/retrofit_bboxes.py airfare-air-india     # one
    python scripts/retrofit_bboxes.py uber1.json uber2      # two
"""

import json
import pathlib
import sys

# evidence_bbox lives in the same scripts/ dir; make it importable
# without requiring the repo to be a package.
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from evidence_bbox import populate_bboxes  # noqa: E402

REPO = pathlib.Path(__file__).resolve().parent.parent
SPIKE = REPO / ".scratch" / "spike"
RECEIPTS = REPO / "receipts"


def _count_populated(obj) -> int:
    n = 0
    if isinstance(obj, dict):
        if "bboxes" in obj:
            n += 1
        for v in obj.values():
            n += _count_populated(v)
    elif isinstance(obj, list):
        for item in obj:
            n += _count_populated(item)
    return n


def main() -> int:
    targets = sorted(SPIKE.glob("*.json"))
    if not targets:
        print(f"no JSONs in {SPIKE}", file=sys.stderr)
        return 1

    if len(sys.argv) > 1:
        wanted = set(sys.argv[1:])
        targets = [
            t for t in targets if t.name in wanted or t.stem in wanted
        ]
        if not targets:
            print(f"no JSONs matched filter {sorted(wanted)}", file=sys.stderr)
            return 1

    counts = {
        "updated": 0,
        "skipped_no_source": 0,
        "skipped_missing_file": 0,
        "errors": 0,
    }

    for path in targets:
        try:
            data = json.loads(path.read_text())
        except json.JSONDecodeError as err:
            print(f"  ERR  {path.name}: invalid JSON: {err}")
            counts["errors"] += 1
            continue

        record = data[0] if isinstance(data, list) else data
        source = record.get("source_filename")
        if not source:
            print(f"  SKIP {path.name}: no source_filename")
            counts["skipped_no_source"] += 1
            continue

        receipt = RECEIPTS / source
        if not receipt.exists():
            print(f"  SKIP {path.name}: {receipt} not found")
            counts["skipped_missing_file"] += 1
            continue

        try:
            populate_bboxes(record, receipt)
        except Exception as err:
            print(f"  ERR  {path.name}: populate_bboxes failed: {err}")
            counts["errors"] += 1
            continue

        n = _count_populated(record)
        path.write_text(json.dumps(data, indent=2) + "\n")
        counts["updated"] += 1
        print(f"  OK   {path.name}  ({n} entries with bboxes)")

    print()
    print("Totals:")
    for k, v in counts.items():
        print(f"  {k:24s} {v}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
