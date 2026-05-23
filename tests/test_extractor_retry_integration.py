"""Integration test for the Stage 21 retry around single_call().

The existing tests/test_extractor_retry.py exercises _is_retryable +
_retry_with_backoff in isolation. This file asserts the wrapper is
actually wired around the live generate_content call site — and that
the structured log event 'extract.retry' fires with the right shape.

REGRESSION SHAPE THIS GUARDS:
  If someone removes the _retry_with_backoff wrap at extractor_lib.py
  line ~238 (e.g. during a refactor) the isolated unit tests still
  pass — but a real Vertex 503 will surface as a hard failure to the
  FA instead of being retried. This test fakes the client + injects
  503, so it'd catch that.
"""

from __future__ import annotations

import io
import json
import sys
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import MagicMock, patch

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from extractor_lib import single_call  # noqa: E402


def _http_error(code: int) -> Exception:
    """Synthesize an exception with `.code` mimicking google.genai.errors."""
    e = Exception(f"synthetic HTTP {code}")
    e.code = code
    return e


def _fake_response(payload):
    """A response.text JSON string that single_call's json.loads can parse."""
    r = MagicMock()
    r.text = json.dumps(payload)
    return r


class TestSingleCallRetryIntegration(unittest.TestCase):
    """Hits single_call() with a stubbed client + stubbed schema loader
    so the only real machinery exercised is the retry wrap."""

    def setUp(self):
        # _load_response_schema otherwise reads a real generated/*.json
        # and validates it as types.Schema. We don't need a real schema
        # — the fake client doesn't inspect it.
        self._schema_patch = patch(
            "extractor_lib._load_response_schema",
            return_value=MagicMock(name="fake_schema"),
        )
        self._schema_patch.start()
        # types.Part.from_bytes also touches google.genai — stub it.
        self._part_patch = patch(
            "google.genai.types.Part.from_bytes",
            return_value=MagicMock(name="fake_part"),
        )
        self._part_patch.start()
        # Same for GenerateContentConfig.
        self._cfg_patch = patch(
            "google.genai.types.GenerateContentConfig",
            return_value=MagicMock(name="fake_cfg"),
        )
        self._cfg_patch.start()

    def tearDown(self):
        self._schema_patch.stop()
        self._part_patch.stop()
        self._cfg_patch.stop()

    def _make_client(self, side_effects):
        """Fake client whose generate_content yields each side_effect in turn."""
        client = MagicMock()
        client.models.generate_content.side_effect = side_effects
        return client

    def _call(self, client):
        # Override the backoff sleep so the test runs in milliseconds.
        with patch("extractor_lib.time.sleep"):
            return single_call(
                client=client,
                model="gemini-fake",
                prompt="extract things",
                response_schema_path=Path("/dev/null"),  # ignored — stubbed
                image_filename="receipt.pdf",
                image_bytes=b"%PDF-1.4 fake bytes",
                mime="application/pdf",
                output_base=Path("/dev/null"),
                diag_label="",
            )

    def test_503_then_success_retries_and_logs(self):
        """One 503 then a valid response — single_call must (a) retry,
        (b) emit the structured 'extract.retry' log event, (c) return
        the parsed payload from the second call."""
        payload = [{"description": "Recovered after retry"}]
        client = self._make_client([
            _http_error(503),
            _fake_response(payload),
        ])

        buf = io.StringIO()
        with redirect_stdout(buf):
            result = self._call(client)

        # Returned the second call's payload.
        self.assertEqual(result, payload)
        # generate_content invoked twice (one fail + one success).
        self.assertEqual(client.models.generate_content.call_count, 2)
        # Structured log event landed on stdout.
        stdout = buf.getvalue()
        self.assertIn('"event": "extract.retry"', stdout,
                      f"expected extract.retry log event, stdout was: {stdout!r}")
        # And it should reference the file in the label so the operator
        # can grep Cloud Logging for the affected receipt.
        self.assertIn("receipt.pdf", stdout)
        # Severity should be WARNING (retry is recoverable; not error).
        self.assertIn('"severity": "WARNING"', stdout)

    def test_400_does_not_retry(self):
        """400 INVALID_ARGUMENT (e.g. schema-too-large, Stage 9c
        regression class) must bubble out after exactly one attempt
        — retrying gives the same answer and wastes quota."""
        client = self._make_client([_http_error(400)])

        buf = io.StringIO()
        with redirect_stdout(buf):
            with self.assertRaises(Exception) as ctx:
                self._call(client)

        self.assertEqual(getattr(ctx.exception, "code", None), 400)
        self.assertEqual(client.models.generate_content.call_count, 1,
                         "400 must not trigger a retry")
        # No extract.retry log event on a non-retryable.
        self.assertNotIn('"event": "extract.retry"', buf.getvalue())

    def test_three_503s_exhausts_and_raises(self):
        """All attempts fail with 503 → single_call re-raises the last
        exception (so the per-file PipelineError path in Stage 11c can
        isolate this receipt from the batch). Should log twice (after
        attempt 1 + 2; no log after the final attempt since there's
        no retry-in-sec to announce)."""
        client = self._make_client([
            _http_error(503),
            _http_error(503),
            _http_error(503),
        ])

        buf = io.StringIO()
        with redirect_stdout(buf):
            with self.assertRaises(Exception) as ctx:
                self._call(client)

        self.assertEqual(getattr(ctx.exception, "code", None), 503)
        self.assertEqual(client.models.generate_content.call_count, 3)
        # Two retry log events (after attempts 1 and 2).
        self.assertEqual(buf.getvalue().count('"event": "extract.retry"'), 2)


if __name__ == "__main__":
    unittest.main()
