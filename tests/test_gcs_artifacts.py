"""Real-GCS tests for scripts/gcs_artifacts.py.

Each test uses a unique test_<uuid> upload_id, then cleans up.
Same skip conditions as test_firestore_reports.py.
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
    from google.cloud import storage  # noqa: F401
    GCS_AVAILABLE = True
except ImportError:
    GCS_AVAILABLE = False


def _gcs_reachable() -> bool:
    if not GCS_AVAILABLE:
        return False
    if not os.environ.get("VERTEX_PROJECT_ID"):
        return False
    try:
        from gcs_artifacts import _get_client, BUCKET_NAME
        client = _get_client()
        # bucket.exists() is a Class B op; trivial cost. Confirms the
        # bucket is accessible to ADC before declaring the suite live.
        return client.bucket(BUCKET_NAME).exists()
    except Exception:
        return False


@unittest.skipUnless(_gcs_reachable(),
                     "GCS unreachable (SDK missing, "
                     "VERTEX_PROJECT_ID unset, ADC not configured, "
                     "or bucket inaccessible)")
class TestGcsArtifacts(unittest.TestCase):

    def setUp(self):
        from gcs_artifacts import (upload_artifact, download_artifact,
                                    list_artifacts, delete_artifacts)
        self.upload_artifact = upload_artifact
        self.download_artifact = download_artifact
        self.list_artifacts = list_artifacts
        self.delete_artifacts = delete_artifacts
        self.upload_id = f"test_{uuid.uuid4().hex[:12]}"
        self.scratch = REPO_ROOT / ".scratch" / "gcs-test" / self.upload_id
        self.scratch.mkdir(parents=True, exist_ok=True)

    def tearDown(self):
        try:
            self.delete_artifacts(self.upload_id)
        except Exception:
            pass
        import shutil
        try:
            shutil.rmtree(self.scratch)
        except Exception:
            pass

    def _make_file(self, name: str, content: bytes = b"hello") -> Path:
        p = self.scratch / name
        p.write_bytes(content)
        return p

    def test_upload_then_list(self):
        a = self._make_file("a.pdf")
        b = self._make_file("b.png", b"binary-data-here")
        self.upload_artifact(self.upload_id, "files", a)
        self.upload_artifact(self.upload_id, "files", b)
        listed = sorted(self.list_artifacts(self.upload_id, "files"))
        self.assertEqual(listed, ["a.pdf", "b.png"])

    def test_upload_then_download_round_trip(self):
        original = self._make_file("receipt.pdf", b"pdf-bytes-redacted")
        self.upload_artifact(self.upload_id, "files", original)
        dest = self.scratch / "round-trip" / "receipt.pdf"
        ok = self.download_artifact(self.upload_id, "files",
                                    "receipt.pdf", dest)
        self.assertTrue(ok, "expected blob to exist after upload")
        self.assertEqual(dest.read_bytes(), b"pdf-bytes-redacted")

    def test_download_returns_false_for_missing_blob(self):
        dest = self.scratch / "nope.pdf"
        ok = self.download_artifact(self.upload_id, "files",
                                    "nonexistent.pdf", dest)
        self.assertFalse(ok)
        self.assertFalse(dest.exists())

    def test_extractions_and_files_are_separate_categories(self):
        src = self._make_file("shared.json", b'{"x": 1}')
        self.upload_artifact(self.upload_id, "files", src)
        self.upload_artifact(self.upload_id, "extractions", src)
        files_listed = self.list_artifacts(self.upload_id, "files")
        extractions_listed = self.list_artifacts(self.upload_id,
                                                  "extractions")
        self.assertEqual(files_listed, ["shared.json"])
        self.assertEqual(extractions_listed, ["shared.json"])

    def test_list_empty_when_nothing_uploaded(self):
        self.assertEqual(self.list_artifacts(self.upload_id, "files"), [])

    def test_delete_removes_all_categories(self):
        src = self._make_file("x.pdf")
        self.upload_artifact(self.upload_id, "files", src)
        self.upload_artifact(self.upload_id, "extractions", src)
        deleted = self.delete_artifacts(self.upload_id)
        self.assertEqual(deleted, 2)
        self.assertEqual(self.list_artifacts(self.upload_id, "files"), [])
        self.assertEqual(self.list_artifacts(self.upload_id,
                                              "extractions"), [])

    def test_upload_with_unicode_sanitized_filename(self):
        # filename has already been sanitized upstream
        # (scripts/local_app_simple.py:sanitize_filename); we just
        # confirm GCS accepts the post-sanitization shape.
        src = self._make_file("receipt_2026_test.pdf")
        self.upload_artifact(self.upload_id, "files", src)
        self.assertIn("receipt_2026_test.pdf",
                      self.list_artifacts(self.upload_id, "files"))


if __name__ == "__main__":
    unittest.main()
