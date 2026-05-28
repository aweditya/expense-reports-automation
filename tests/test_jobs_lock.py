"""Regression: JOBS_LOCK must be reentrant.

`_update_file` in scripts/local_app_simple.py holds JOBS_LOCK across a
read-modify-write and calls `_get_job` / `_set_job` inside it, which
re-acquire the lock. A plain `threading.Lock` deadlocks that nesting on
the in-memory jobs path (USE_FIRESTORE_JOBS=0) — the symptom: an upload
hangs at the first extraction callback with no subprocess ever spawned.
The Firestore path never touches this lock, so prod never hit it. This
guards the RLock fix.
"""

from __future__ import annotations

import sys
import threading
import unittest
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import local_app_simple as app  # noqa: E402


class TestJobsLockReentrancy(unittest.TestCase):
    def test_set_and_get_job_nest_under_held_jobs_lock(self) -> None:
        # Force the in-memory path (the one with the lock) regardless of
        # the runner's env, then mirror _update_file's nesting. Run in a
        # thread with a join timeout so a regression (plain Lock) fails
        # the assertion instead of hanging the whole suite.
        done = threading.Event()

        def nested() -> None:
            with app.JOBS_LOCK:
                app._set_job("reentrancy-test", phase="extract")
                snap = app._get_job("reentrancy-test")
                if snap.get("phase") == "extract":
                    done.set()

        with mock.patch.object(app, "USE_FIRESTORE_JOBS", False):
            t = threading.Thread(target=nested, daemon=True)
            t.start()
            t.join(timeout=5)

        app.JOBS.pop("reentrancy-test", None)
        self.assertTrue(
            done.is_set(),
            "JOBS_LOCK is not reentrant — _set_job/_get_job deadlocked "
            "under a held lock. Revert to threading.RLock().",
        )


if __name__ == "__main__":
    unittest.main()
