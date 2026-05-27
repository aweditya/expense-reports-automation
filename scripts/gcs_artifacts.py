"""GCS-backed storage for the binary + structured artifacts that
Firestore can't hold cheaply:

  - source PDFs / images uploaded by the FA (binary, 100s of KB)
  - per-receipt extraction JSONs produced by Gemini (structured but
    too many small objects to fit one Firestore doc)

Mirror of `firestore_reports.py` for the artifact tier.

Layout:
    gs://<bucket>/uploads/<upload_id>/files/<sanitized-filename>
    gs://<bucket>/uploads/<upload_id>/extractions/<basename>.json

Callers must invoke `upload_artifact` after the file is written
to local disk; this module does not write to disk. Disk remains
authoritative for in-flight reads; GCS is the recovery source on
container recycle.

Gated by env var `USE_GCS_ARTIFACTS` in the caller — this module
itself does no gating, just operations.

Cost: ~10 uploads/day × ~5 files × 200 KB = 10 MB/day. Storage
$0.020/GB → ~$0.006/month. Class A ops $0.05/10K → trivial.
"""

from __future__ import annotations

import os
import threading
from pathlib import Path
from typing import Any

_client: Any = None
_client_lock = threading.Lock()


def _get_client():
    global _client
    with _client_lock:
        if _client is None:
            from google.cloud import storage
            project = (
                os.environ.get("VERTEX_PROJECT_ID")
                or os.environ.get("GOOGLE_CLOUD_PROJECT")
                or os.environ.get("GCLOUD_PROJECT")
            )
            _client = storage.Client(project=project)
        return _client


BUCKET_NAME = os.environ.get("GCS_ARTIFACTS_BUCKET",
                              "soe-agile-agents-expense-reports-state")


def _blob_path(upload_id: str, category: str, filename: str) -> str:
    """Object key under the bucket. `category` is "files" (source PDFs)
    or "extractions" (per-receipt JSONs). Caller passes the already-
    sanitized filename — we don't re-sanitize because the upload flow
    has already done that."""
    return f"uploads/{upload_id}/{category}/{filename}"


def upload_artifact(upload_id: str, category: str,
                    local_path: Path) -> None:
    """Upload one file to GCS at the per-upload layout location.
    Idempotent: re-upload overwrites the existing blob."""
    bucket = _get_client().bucket(BUCKET_NAME)
    blob = bucket.blob(_blob_path(upload_id, category, local_path.name))
    blob.upload_from_filename(str(local_path))


def download_artifact(upload_id: str, category: str,
                      filename: str, dest_path: Path) -> bool:
    """Download one file from GCS to local disk. Returns False if
    the blob doesn't exist (caller decides whether that's a 404 or
    a "cache cold, re-extract" situation). Creates parent dirs."""
    bucket = _get_client().bucket(BUCKET_NAME)
    blob = bucket.blob(_blob_path(upload_id, category, filename))
    if not blob.exists():
        return False
    dest_path.parent.mkdir(parents=True, exist_ok=True)
    blob.download_to_filename(str(dest_path))
    return True


def list_artifacts(upload_id: str, category: str) -> list[str]:
    """List filenames under uploads/<id>/<category>/. Returns
    basenames only (strips the prefix), in arbitrary order. Empty
    list when nothing exists — never raises for a missing
    'directory' (GCS has no real directories)."""
    bucket = _get_client().bucket(BUCKET_NAME)
    prefix = f"uploads/{upload_id}/{category}/"
    return [b.name.removeprefix(prefix)
            for b in bucket.list_blobs(prefix=prefix)]


def delete_artifacts(upload_id: str) -> int:
    """Delete every blob under uploads/<id>/. Returns the count
    deleted. Cleanup tooling + tests; the live pipeline relies on
    bucket lifecycle rules for retention (set in the GCP console).
    """
    bucket = _get_client().bucket(BUCKET_NAME)
    prefix = f"uploads/{upload_id}/"
    count = 0
    for blob in bucket.list_blobs(prefix=prefix):
        blob.delete()
        count += 1
    return count
