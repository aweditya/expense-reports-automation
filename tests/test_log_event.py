"""Unit tests for scripts/log_event.py (Stage 22).

Asserts the emitted lines are valid JSON, carry the expected fields,
and route to the right stream (INFO/WARNING → stdout, ERROR → stderr).
"""

from __future__ import annotations

import io
import json
import sys
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from log_event import log_error, log_event, log_warning  # noqa: E402


def _capture_stdout(fn, *args, **kwargs) -> str:
    buf = io.StringIO()
    with redirect_stdout(buf):
        fn(*args, **kwargs)
    return buf.getvalue()


def _capture_stderr(fn, *args, **kwargs) -> str:
    buf = io.StringIO()
    with redirect_stderr(buf):
        fn(*args, **kwargs)
    return buf.getvalue()


class TestLogEvent(unittest.TestCase):
    def test_emits_valid_json_with_severity_and_event(self):
        out = _capture_stdout(log_event, "extract.start", upload_id="abc")
        payload = json.loads(out.strip())
        self.assertEqual(payload["severity"], "INFO")
        self.assertEqual(payload["event"], "extract.start")
        self.assertEqual(payload["upload_id"], "abc")
        self.assertIn("timestamp", payload)
        self.assertIsInstance(payload["timestamp"], float)

    def test_arbitrary_fields_pass_through(self):
        out = _capture_stdout(
            log_event, "extract.done",
            upload_id="u1", filename="receipt.pdf",
            kind="meal", duration_ms=12345, n_attempts=2,
        )
        payload = json.loads(out.strip())
        self.assertEqual(payload["filename"], "receipt.pdf")
        self.assertEqual(payload["duration_ms"], 12345)
        self.assertEqual(payload["n_attempts"], 2)

    def test_non_json_serializable_fields_stringified(self):
        # default=str in json.dumps means Path / datetime / Exception
        # render via str() instead of crashing the logger.
        out = _capture_stdout(log_event, "path.event",
                              path=Path("/tmp/x"), err=ValueError("boom"))
        payload = json.loads(out.strip())
        self.assertEqual(payload["path"], "/tmp/x")
        self.assertEqual(payload["err"], "boom")

    def test_writes_to_stdout_not_stderr(self):
        # log_event must not pollute stderr — Cloud Logging treats
        # stderr lines as ERROR severity by default, which would
        # mis-classify routine INFO events.
        err = _capture_stderr(log_event, "test", x=1)
        self.assertEqual(err, "")


class TestLogError(unittest.TestCase):
    def test_emits_to_stderr_with_error_severity(self):
        err = _capture_stderr(log_error, "pipeline.fatal",
                              upload_id="u1", error="boom")
        payload = json.loads(err.strip())
        self.assertEqual(payload["severity"], "ERROR")
        self.assertEqual(payload["event"], "pipeline.fatal")
        self.assertEqual(payload["error"], "boom")

    def test_does_not_pollute_stdout(self):
        out = _capture_stdout(log_error, "test", x=1)
        self.assertEqual(out, "")


class TestLogWarning(unittest.TestCase):
    def test_emits_to_stdout_with_warning_severity(self):
        out = _capture_stdout(log_warning, "fx.degraded",
                              currency="ZZX", reason="unsupported")
        payload = json.loads(out.strip())
        self.assertEqual(payload["severity"], "WARNING")
        self.assertEqual(payload["event"], "fx.degraded")
        self.assertEqual(payload["currency"], "ZZX")


class TestJSONShape(unittest.TestCase):
    def test_one_line_per_call(self):
        # Cloud Logging treats each newline-terminated line as a
        # separate log entry. Multiple emits = multiple entries; one
        # emit must produce exactly one newline.
        out = _capture_stdout(log_event, "single.line", x=1)
        self.assertEqual(out.count("\n"), 1)
        # The line itself must contain no internal newlines (would
        # split the JSON across multiple Cloud Logging entries).
        self.assertNotIn("\n", out.rstrip("\n"))


if __name__ == "__main__":
    unittest.main()
