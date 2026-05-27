"""Firestore-backed reports storage.

Persists the reduced report (the FA-edited canonical state), the
form fieldset, and the edit history to Firestore. Mirror of
`firestore_jobs.py` for the live-progress dict.

Today the per-upload state lives in `.scratch/uploads/<id>/`:
  reduced/report.json   ← canonical FA-edited state
  fa_input.json         ← the form fieldset
  edit_history.json     ← undo stack
The container-local disk is wiped on every Cloud Run restart, so a
24-hour-old workbench URL just 404s. Firestore-backed reports
survive container recycle — the FA can come back tomorrow.

Stage 2a (this module) lands the WRITE side: every upload + every
edit dual-writes Firestore alongside the existing disk write.
Stage 2c will land the READ side: when the disk cache is missing,
re-hydrate from Firestore and re-render the workbench.

Gated by env var `USE_FIRESTORE_REPORTS` so the write can be
shipped dark + rolled back by flipping the gate.

Schema:
  reports/{upload_id} = {
    report:    <full report.json dict>,
    fa_input:  <fa_input.json dict or null>,
    history:   <list of edit-history entries; capped at 50>,
    created_at: timestamp,
    updated_at: timestamp,
    ttl:        timestamp (90 days from creation; FA-editable reports
                live longer than live-progress JOBS),
  }

Cost: ~10 uploads/day × (1 write at completion + ~5 writes per edit
session) × 30 days = ~1500 writes/month. At $0.18/100K writes,
$0.003/month. Storage: ~50 KB/doc × 300 docs/quarter = 15 MB,
trivial.

ADC: same as firestore_jobs.py — Cloud Run metadata server, or
`gcloud auth application-default login` locally.
"""

from __future__ import annotations

import datetime as dt
import json
import os
import threading
from typing import Any

_client: Any = None
_client_lock = threading.Lock()


def _get_client():
    global _client
    with _client_lock:
        if _client is None:
            from google.cloud import firestore
            project = (
                os.environ.get("VERTEX_PROJECT_ID")
                or os.environ.get("GOOGLE_CLOUD_PROJECT")
                or os.environ.get("GCLOUD_PROJECT")
            )
            _client = firestore.Client(project=project, database="(default)")
        return _client


COLLECTION = "reports"
TTL_DAYS = 90
HISTORY_CAP = 50


def set_report(upload_id: str, *, report: dict,
               fa_input: dict | None = None,
               history: list | None = None,
               filed_by_sunet: str | None = None) -> None:
    """Full-merge upsert of reports/{upload_id}. Pass the full report
    + (optionally) fa_input + history; the caller owns assembly.

    The report payload is JSON-encoded into a single string field
    (`report_json`) because Firestore rejects arrays-of-arrays as
    "invalid nested entity" — and our report carries bbox coordinate
    arrays from the OCR-grounding layer. JSON-string wire format
    sidesteps every Firestore shape constraint at zero practical cost
    (~50 KB string is well under the 1 MB doc limit). fa_input +
    history are flat structures and pass through natively.

    Sets `updated_at` on every write. Sets `created_at` + `ttl` on
    first write only. History is capped at HISTORY_CAP entries
    (oldest dropped) — keeps doc size bounded and matches FA usage
    patterns (no realistic session generates >50 edits).
    """
    now = dt.datetime.now(dt.timezone.utc)
    capped_history = (history or [])[-HISTORY_CAP:]
    payload: dict[str, Any] = {
        "report_json": json.dumps(report, ensure_ascii=False),
        "fa_input": fa_input,
        "history": capped_history,
        "updated_at": now,
    }
    if filed_by_sunet:
        payload["filed_by_sunet"] = filed_by_sunet
    doc_ref = _get_client().collection(COLLECTION).document(upload_id)
    snap = doc_ref.get(field_paths=["created_at"])
    if not snap.exists or "created_at" not in (snap.to_dict() or {}):
        payload["created_at"] = now
        payload["ttl"] = now + dt.timedelta(days=TTL_DAYS)
    doc_ref.set(payload, merge=True)


def get_report(upload_id: str) -> dict | None:
    """Snapshot read. Returns the full doc with `report` decoded back
    to a dict (the wire format stores `report_json` as a string —
    callers see the same dict shape they passed to set_report).
    Returns None if no document exists; caller falls back to the disk
    cache."""
    doc_ref = _get_client().collection(COLLECTION).document(upload_id)
    snap = doc_ref.get()
    if not snap.exists:
        return None
    data = snap.to_dict() or {}
    for f in ("created_at", "updated_at", "ttl"):
        data.pop(f, None)
    if "report_json" in data:
        data["report"] = json.loads(data.pop("report_json"))
    return data


def delete_report(upload_id: str) -> None:
    """Explicit deletion. Not used by the live pipeline (TTL handles
    cleanup) but handy for tests + cleanup tooling."""
    _get_client().collection(COLLECTION).document(upload_id).delete()


def list_reports(filed_by_sunet: str | None = None,
                 limit: int = 100) -> list[dict]:
    """List per-upload summaries for the dashboard. Returns dicts with
    upload_id, updated_at, fa_input, filed_by_sunet, line_count,
    total_usd — enough to render a one-row-per-report table without
    re-fetching the full payload for each entry.

    If `filed_by_sunet` is set, filters to only that filer's reports
    (Firestore single-field equality, no composite index needed) —
    this is the dashboard scoping path. Pre-IAP reports without a
    filed_by_sunet field are EXCLUDED from any scoped query (you only
    see reports you filed after the scoping landed). Pass None to
    fetch the most recent `limit` reports across all filers
    (admin-only path; the dashboard never does this for real FAs).
    Always sorted by updated_at descending (client-side; at our scale
    of a few hundred docs this is cheaper than a composite index)."""
    import json as _json
    coll = _get_client().collection(COLLECTION)
    if filed_by_sunet:
        query = coll.where(filter=_filter_eq("filed_by_sunet",
                                              filed_by_sunet))
    else:
        query = coll
    results: list[dict] = []
    for snap in query.stream():
        data = snap.to_dict() or {}
        try:
            report = _json.loads(data.get("report_json") or "{}")
        except (ValueError, TypeError):
            report = {}
        lines = report.get("transaction_lines") or []
        summary = (report.get("transaction_summary") or {})
        total_node = summary.get("total_usd") or {}
        results.append({
            "upload_id": snap.id,
            "updated_at": data.get("updated_at"),
            "fa_input": data.get("fa_input") or {},
            "filed_by_sunet": data.get("filed_by_sunet"),
            "line_count": len(lines),
            "total_usd": total_node.get("value"),
        })
    results.sort(key=lambda r: (r["updated_at"] or 0), reverse=True)
    return results[:limit]


def _filter_eq(field_path: str, value):
    """Build a Firestore FieldFilter for `field == value`. The SDK
    deprecated positional .where() in favor of the FieldFilter object;
    isolating it here keeps callers tidy + makes future API drift a
    one-line change."""
    from google.cloud.firestore_v1.base_query import FieldFilter
    return FieldFilter(field_path, "==", value)
