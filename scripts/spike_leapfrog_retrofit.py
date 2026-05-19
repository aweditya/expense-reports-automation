#!/usr/bin/env python3
"""Take the leapfrog spike's output and overlay its resolved bboxes
into the cached per-doc JSONs, so the workbench renders LEAPFROG
halos (instead of text-matcher halos) WITHOUT touching production
extractors.

This is a spike-grade visualization tool — lets the user click
through the workbench and see whether token-id-grounded bboxes
actually land on the right text in the source documents. Spike
maps a kind-agnostic 6-field schema (date / total_amount /
currency / vendor_name / address / expense_type) to the
corresponding evidence paths in the production schema.

Fields the spike didn't extract are left untouched — they keep
their existing (text-matcher) bboxes. So the workbench shows a
MIX of leapfrog and current-architecture halos; the leapfrog ones
are the test subject.

Inputs:
  .scratch/audit/leapfrog_spike.json   (from spike_leapfrog.py)
  .scratch/spike/*.json                (per-doc cached JSONs)

Effect: mutates the per-doc JSONs in place. Both inputs are
gitignored. Re-run scripts/retrofit_bboxes.py if you want to
restore text-matcher bboxes.
"""

import json
import pathlib
import sys
from collections import defaultdict

REPO = pathlib.Path(__file__).resolve().parent.parent
SPIKE_JSON = REPO / ".scratch" / "audit" / "leapfrog_spike.json"
CACHED_DIR = REPO / ".scratch" / "spike"

# Map spike's kind-agnostic field name -> list of cached-JSON
# evidence-path SUFFIXES it might apply to. The path-walker matches
# any path ending in one of these suffixes (so it works across
# kinds: meal/lodging/airfare/transport).
SPIKE_TO_EVIDENCE_PATHS = {
    "date": ["common.date"],
    "total_amount": [
        "common.line_amount_usd",
        "airfare_details.ticket_amount",
        # NOT meal_details.tip_amount etc — those are sub-amounts
    ],
    "currency": ["common.original_currency", "extras.printed_currency"],
    "expense_type": ["common.expense_type"],
    "address": ["extras.merchant_address"],
    "vendor_name": [
        "meal_details.venue_name",
        "lodging_details.hotel_name",
        "airfare_details.airline",
        "ground_transport_details.service_provider",
    ],
}


def index_spike(spike_records: list[dict]) -> dict[str, dict[str, list[float]]]:
    """Return {source_filename: {spike_field_name: bbox}}.
    Only includes entries whose verifier passed AND have a resolved
    bbox — those are the trustworthy leapfrog grounding signals.
    """
    out: dict[str, dict[str, list[float]]] = defaultdict(dict)
    for rec in spike_records:
        if "fields" not in rec:
            continue
        fname = rec.get("source_filename")
        if not fname:
            continue
        for f in rec["fields"]:
            if not f.get("verifier_passed"):
                continue
            bbox = f.get("bbox")
            if not bbox:
                continue
            out[fname][f["name"]] = bbox
    return dict(out)


def overlay_record(record: dict, spike_bboxes_for_source: dict[str, list[float]]) -> int:
    """Walk a cached per-doc record; replace bboxes on evidence whose
    field-path suffix matches a spike-known mapping AND the spike
    has a bbox for the corresponding spike field name on this
    receipt. Returns the count of evidence entries that got
    overwritten."""
    count = 0

    # Walk the record collecting all (path, meta) pairs at every
    # leaf. Same shape walker as in spike_evidence_audit.py.
    def _walk(obj, path: str):
        nonlocal count
        if isinstance(obj, dict):
            meta = obj.get("_meta")
            if isinstance(meta, dict) and "evidence" in meta:
                for spike_field, suffixes in SPIKE_TO_EVIDENCE_PATHS.items():
                    if any(path.endswith(s) for s in suffixes):
                        bbox = spike_bboxes_for_source.get(spike_field)
                        if not bbox:
                            continue
                        # Replace bboxes on the FIRST document_span
                        # entry; matches the workbench's "first
                        # eligible evidence" icon emission.
                        for ev in meta.get("evidence") or []:
                            if not isinstance(ev, dict):
                                continue
                            if ev.get("kind") != "document_span":
                                continue
                            if not (ev.get("quote") or "").strip():
                                continue
                            ev["bboxes"] = [bbox]
                            count += 1
                            break
                        break
            for k, v in obj.items():
                if k == "_meta":
                    continue
                _walk(v, f"{path}.{k}" if path else k)
        elif isinstance(obj, list):
            for i, item in enumerate(obj):
                _walk(item, f"{path}[{i}]")

    _walk(record, "")
    return count


def main() -> int:
    if not SPIKE_JSON.exists():
        print(f"no spike output at {SPIKE_JSON} — run spike_leapfrog.py first",
              file=sys.stderr)
        return 1
    spike_records = json.loads(SPIKE_JSON.read_text())
    spike_index = index_spike(spike_records)
    print(f"spike has bboxes for {len(spike_index)} receipts")

    cached = sorted(CACHED_DIR.glob("*.json"))
    if not cached:
        print(f"no JSONs in {CACHED_DIR}", file=sys.stderr)
        return 1

    total_overlaid = 0
    receipts_touched = 0
    for path in cached:
        try:
            data = json.loads(path.read_text())
        except json.JSONDecodeError as err:
            print(f"  ERR  {path.name}: {err}")
            continue
        record = data[0] if isinstance(data, list) else data
        source = record.get("source_filename")
        if not source:
            continue
        spike_for_source = spike_index.get(source) or {}
        if not spike_for_source:
            print(f"  SKIP {path.name}  (no spike data for {source!r})")
            continue
        n = overlay_record(record, spike_for_source)
        if n:
            path.write_text(json.dumps(data, indent=2) + "\n")
            print(f"  OK   {path.name}  ({n} evidence entries overlaid with leapfrog bboxes)")
            total_overlaid += n
            receipts_touched += 1

    print()
    print(f"overlaid {total_overlaid} bboxes across {receipts_touched} receipts")
    print("re-render workbench: cargo run --quiet --bin render_workbench_from_report -- --source-docs-url-prefix /receipts/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
