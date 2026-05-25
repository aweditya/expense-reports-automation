"""Firestore-backed JOBS storage (Durable-store Phase 1).

Replaces the in-memory `JOBS` dict in `scripts/local_app_simple.py`
with a Firestore collection `jobs/{upload_id}`. The selection is
gated by env var `USE_FIRESTORE_JOBS` (set in deploy/cloudbuild.yaml)
so this code can ship dark + be rolled back by flipping the gate off.

Why: today a Cloud Run container recycle (idle for >15min, deploy,
health check fail) wipes the in-memory `JOBS` dict mid-upload. The
SSE endpoint returns `phase=lost` and the FA has to re-upload.
Firestore-backed JOBS survives container recycle — the FA can
reload, switch tabs, or reconnect from another device and pick up
where the live progress left off.

Interface mirrors `local_app_simple._set_job` / `_get_job` exactly
so the swap in the dispatcher is one line.

Cost shape (free tier covers our load):
  - 50K reads/day, 20K writes/day, 1 GB storage in the free tier
  - Our load: ~10 uploads/day × ~12 phase transitions (writes) +
    ~600 SSE polls (reads) at 0.5s polling for ~5-min upload =
    120 writes/day + 6K reads/day. Well under free tier.
  - Beyond free tier: $0.06/100K reads, $0.18/100K writes.

TTL: documents expire 7 days after creation via the `ttl` field
(set by `set_job`). Requires a one-time TTL policy in the GCP
console for collection `jobs` field `ttl` — see docs/durable-
store-plan.md §6 Phase 1 setup. Without the policy, documents
just linger; cost is still trivial (<1MB total).

ADC: works on Cloud Run via the metadata server, and locally
after `gcloud auth application-default login`. SDK picks up
credentials automatically when none are passed.
"""

from __future__ import annotations

import datetime as dt
import os
import threading
from typing import Any

# Lazy import: google-cloud-firestore is in deploy/requirements.txt but
# may not be installed in every dev env. Importing at module load time
# would break any test that doesn't need Firestore. Pay the import cost
# only when a caller actually uses the helpers.
_client: Any = None
_client_lock = threading.Lock()


def _get_client():
    """Lazy Firestore client. Builds the client on first call, reuses
    it thereafter (thread-safe). Reads the GCP project from the
    standard env var; falls back to ADC default."""
    global _client
    with _client_lock:
        if _client is None:
            from google.cloud import firestore
            project = (
                os.environ.get("VERTEX_PROJECT_ID")
                or os.environ.get("GOOGLE_CLOUD_PROJECT")
                or os.environ.get("GCLOUD_PROJECT")
            )
            # database=(default) matches what `gcloud firestore
            # databases create` produced on 2026-05-25.
            _client = firestore.Client(project=project, database="(default)")
        return _client


COLLECTION = "jobs"
TTL_DAYS = 7


def set_job(upload_id: str, **updates) -> None:
    """Partial-merge update of jobs/{upload_id}. Matches the
    in-memory `_set_job(**updates)` interface.

    Sets `updated_at` on every write. Sets `created_at` + `ttl`
    only on the first write (the merge=True semantics make
    overwriting safe but redundant).
    """
    if not updates:
        return
    now = dt.datetime.now(dt.timezone.utc)
    doc_ref = _get_client().collection(COLLECTION).document(upload_id)
    payload = dict(updates)
    payload["updated_at"] = now
    # On first write we need created_at + ttl. Cheaper to check
    # exists() once than to read-then-write every time. The
    # get(fields=...) trick only fetches metadata, not the full doc.
    snap = doc_ref.get(field_paths=["created_at"])
    if not snap.exists or "created_at" not in (snap.to_dict() or {}):
        payload["created_at"] = now
        payload["ttl"] = now + dt.timedelta(days=TTL_DAYS)
    doc_ref.set(payload, merge=True)


def get_job(upload_id: str) -> dict:
    """Snapshot read. Returns {} if no document exists. Strips
    bookkeeping fields (created_at/updated_at/ttl) so the SSE
    serializer sees the same shape as the in-memory version."""
    doc_ref = _get_client().collection(COLLECTION).document(upload_id)
    snap = doc_ref.get()
    if not snap.exists:
        return {}
    data = snap.to_dict() or {}
    # Strip Firestore-only fields so the SSE event matches the
    # in-memory shape exactly. SSE consumers don't need timestamps.
    for f in ("created_at", "updated_at", "ttl"):
        data.pop(f, None)
    return data


def delete_job(upload_id: str) -> None:
    """Explicit deletion. Not used by the live pipeline (TTL handles
    cleanup) but handy for tests + cleanup tooling."""
    _get_client().collection(COLLECTION).document(upload_id).delete()
