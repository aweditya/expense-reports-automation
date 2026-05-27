"""Real-Firestore tests for scripts/firestore_jobs.py.

These tests exercise the actual Firestore database (the default in
soe-agile-agents, created 2026-05-25). Each test writes + reads a
document with a unique test_<uuid> id, then cleans up. Cost per run:
trivial (well under free tier).

SKIPPED when:
  - google-cloud-firestore isn't installed (dev env without the SDK)
  - VERTEX_PROJECT_ID env var isn't set (no ADC project hint)
  - Firestore isn't reachable (network, ADC missing, etc.)
"""

from __future__ import annotations

import os
import sys
import unittest
import uuid
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

try:
    from google.cloud import firestore  # noqa: F401
    FIRESTORE_AVAILABLE = True
except ImportError:
    FIRESTORE_AVAILABLE = False


def _firestore_reachable() -> bool:
    """Quick liveness check before declaring the suite runnable."""
    if not FIRESTORE_AVAILABLE:
        return False
    if not os.environ.get("VERTEX_PROJECT_ID"):
        return False
    try:
        from firestore_jobs import _get_client
        # Touching the client triggers ADC + project detection. A
        # genuinely-broken env will raise here; reachable envs return
        # quickly even though we haven't done any reads.
        _get_client()
        return True
    except Exception:
        return False


@unittest.skipUnless(_firestore_reachable(),
                     "Firestore unreachable (SDK missing, "
                     "VERTEX_PROJECT_ID unset, or ADC not configured)")
class TestFirestoreJobs(unittest.TestCase):
    """Each test uses a unique test-prefixed upload_id so parallel
    runs don't collide + we can grep test docs for cleanup. tearDown
    deletes the doc."""

    def setUp(self):
        # Lazy import inside the test so the module gracefully no-ops
        # when SDK isn't present (the skipUnless above guards this).
        from firestore_jobs import set_job, get_job, delete_job
        self.set_job = set_job
        self.get_job = get_job
        self.delete_job = delete_job
        self.upload_id = f"test_{uuid.uuid4().hex[:12]}"

    def tearDown(self):
        try:
            self.delete_job(self.upload_id)
        except Exception:
            pass  # cleanup is best-effort

    def test_get_returns_empty_for_unknown_id(self):
        # Just-generated UUID has no document; get returns {}.
        self.assertEqual(self.get_job(self.upload_id), {})

    def test_set_then_get_round_trip(self):
        self.set_job(self.upload_id, phase="extract", current=0)
        got = self.get_job(self.upload_id)
        self.assertEqual(got["phase"], "extract")
        self.assertEqual(got["current"], 0)
        # Bookkeeping fields stripped from get_job output.
        self.assertNotIn("created_at", got)
        self.assertNotIn("updated_at", got)
        self.assertNotIn("ttl", got)

    def test_set_is_partial_merge_not_overwrite(self):
        # First write: phase + current.
        self.set_job(self.upload_id, phase="extract", current=0)
        # Second write: only phase. current should survive.
        self.set_job(self.upload_id, phase="reduce")
        got = self.get_job(self.upload_id)
        self.assertEqual(got["phase"], "reduce")
        self.assertEqual(got["current"], 0,
                         "partial update must not wipe other fields")

    def test_set_no_op_when_no_updates(self):
        # set_job(**{}) should be a no-op; doc shouldn't get created
        # just to stamp updated_at.
        self.set_job(self.upload_id)
        self.assertEqual(self.get_job(self.upload_id), {})

    def test_set_handles_nested_dict_files_list(self):
        # The real JOBS uses a list-of-dicts for files. Firestore
        # supports nested types natively; this regression guards
        # against accidentally JSON-encoding lists somewhere.
        files = [
            {"name": "a.pdf", "kind": "meal", "status": "done"},
            {"name": "b.pdf", "kind": "transport", "status": "extracting"},
        ]
        self.set_job(self.upload_id, files=files, current=1)
        got = self.get_job(self.upload_id)
        self.assertEqual(got["files"], files)
        self.assertEqual(got["current"], 1)

    def test_delete_removes_document(self):
        self.set_job(self.upload_id, phase="done")
        self.assertEqual(self.get_job(self.upload_id).get("phase"), "done")
        self.delete_job(self.upload_id)
        self.assertEqual(self.get_job(self.upload_id), {})

    def test_delete_unknown_id_is_safe(self):
        # Firestore's delete() is idempotent — deleting a
        # non-existent doc is a no-op, not an error.
        self.delete_job(f"never-exists-{uuid.uuid4().hex[:8]}")


if __name__ == "__main__":
    unittest.main()
