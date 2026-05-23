#!/usr/bin/env python3
"""Shared infrastructure for per-kind Gemini extractor scripts.

Per-kind extractor scripts (`extract_meal.py`, `extract_transport.py`,
`extract_lodging.py`) all share the same shape: parse CLI args, set up a
Vertex AI Gemini client via ADC, load an image, send one or more
structured-output calls, write the parsed JSON. The only per-kind
variation is the **prompt text(s)**, the **response_schema path(s)**,
and how to combine multiple call results.

This module owns:

- CLI parsing (`parse_args`)
- MIME detection (`detect_mime_type`)
- One Gemini structured-output call (`single_call`) — the primitive
  every extractor uses, once for single-call kinds (meal, transport)
  or N times in parallel for multi-call kinds (lodging).
- The "single call → write" full flow for single-call kinds
  (`run_extraction`).

Adding a new single-call kind is a ~30-line script: prompt + schema
path + one call into `run_extraction`. Adding a multi-call kind (only
lodging today, due to a Vertex schema property-count ceiling that
trips above ~5 detail-block fields) requires the script to orchestrate
`single_call` itself — see `scripts/extract_lodging.py` for the
pattern.

Auth is Application Default Credentials (ADC). On Cloud Run that's
the runtime service account via the metadata server. Locally:
    gcloud auth application-default login
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any

from evidence_bbox import (
    format_tokens_for_prompt,
    ocr_document,
    populate_bboxes,
)
from log_event import log_event, log_warning


DEFAULT_MODEL = "gemini-3-flash-preview"
DEFAULT_LOCATION = "global"
DEFAULT_MAX_OUTPUT_TOKENS = 65536

# Stage 21: retry transient Vertex failures (rate limit, 5xx, network
# blips) with exponential backoff. Don't retry permanent failures (400
# INVALID_ARGUMENT means our schema is wrong; 403 means auth is wrong;
# retrying won't help and just wastes time + quota). 3 attempts means
# we tolerate up to ~7s of transient flakiness (1+2+4 backoff) before
# bubbling the failure up to the per-file PipelineError path, which
# Stage 11c isolates from the rest of the batch.
RETRYABLE_HTTP_STATUSES = {408, 429, 500, 502, 503, 504}
RETRY_MAX_ATTEMPTS = 3
RETRY_BACKOFF_BASE_SEC = 1.0


def _is_retryable(exc: BaseException) -> bool:
    """Decide whether to retry a Gemini call after the given exception.

    Retryable: HTTP 408/429/5xx (transient server-side issues), TCP
    timeouts, generic network errors. NOT retryable: 4xx other than
    429 (the request itself is bad — retrying makes it bad again);
    JSONDecodeError (model truncated, not transient); auth errors."""
    # google.genai uses a ClientError class with a `.code` attribute.
    code = getattr(exc, "code", None)
    if isinstance(code, int) and code in RETRYABLE_HTTP_STATUSES:
        return True
    # Network-level errors (DNS, TCP timeout, broken pipe).
    if isinstance(exc, (TimeoutError, ConnectionError)):
        return True
    # google.api_core wraps some transient errors as ServiceUnavailable /
    # InternalServerError / DeadlineExceeded. Detect by name to avoid
    # importing google.api_core just for this check.
    name = type(exc).__name__
    if name in {"ServiceUnavailable", "InternalServerError",
                "DeadlineExceeded", "GoogleAPIError", "RetryError"}:
        return True
    return False


def _retry_with_backoff(fn, *, label: str):
    """Run `fn()`. On retryable exceptions, sleep + retry with
    exponential backoff. Re-raise the last exception when attempts
    are exhausted, or any non-retryable exception immediately.

    `label` is used only for stderr logging so the operator can see
    "retrying meal extract for foo.pdf after 503" in Cloud Logging."""
    last_exc: BaseException | None = None
    for attempt in range(1, RETRY_MAX_ATTEMPTS + 1):
        try:
            return fn()
        except Exception as exc:  # noqa: BLE001 — we re-raise non-retryables
            if not _is_retryable(exc):
                raise
            last_exc = exc
            if attempt == RETRY_MAX_ATTEMPTS:
                break
            delay = RETRY_BACKOFF_BASE_SEC * (2 ** (attempt - 1))
            log_warning("extract.retry",
                        label=label, error_type=type(exc).__name__,
                        attempt=attempt, max_attempts=RETRY_MAX_ATTEMPTS,
                        retry_in_sec=delay)
            time.sleep(delay)
    # All retries exhausted — re-raise the last exception.
    assert last_exc is not None
    raise last_exc


class GeminiCallFailed(Exception):
    """One Gemini structured-output call failed (truncation, schema rejection,
    or any other JSON-parse failure of the response).

    Diagnostics (raw response text + structured failure info) are written
    to disk before this is raised — the message includes the file paths
    so the FA-facing error page and the operator both have something to
    chase.
    """


def parse_args(description: str | None = None) -> argparse.Namespace:
    """Parse the standard per-kind extractor CLI.

    All per-kind extractors take the same flags: --image (input file),
    --output (where to write the JSON), and the GCP project/location/
    model knobs. Defaults pull from environment variables so Cloud Run's
    `--set-env-vars` configuration is the only place that needs to know
    project IDs.
    """
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("--image", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--project",
        default=os.environ.get("VERTEX_PROJECT_ID"),
        help="GCP project (defaults to $VERTEX_PROJECT_ID; Cloud Run sets it "
        "via --set-env-vars in deploy/cloudbuild.yaml).",
    )
    parser.add_argument(
        "--location",
        default=os.environ.get("VERTEX_LOCATION", DEFAULT_LOCATION),
    )
    parser.add_argument(
        "--model",
        default=os.environ.get("VERTEX_GEMINI_MODEL", DEFAULT_MODEL),
    )
    return parser.parse_args()


def detect_mime_type(path: Path) -> str:
    suffix = path.suffix.lower()
    return {
        ".pdf": "application/pdf",
        ".png": "image/png",
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
    }.get(suffix) or sys.exit(f"unsupported image format: {suffix}")


def _load_response_schema(path: Path):
    """Load a generated response_schema JSON file as a google-genai Schema."""
    from google.genai import types

    schema_dict = json.loads(path.read_text())
    return types.Schema.model_validate(schema_dict)


def single_call(
    *,
    client,
    model: str,
    prompt: str,
    response_schema_path: Path,
    image_filename: str,
    image_bytes: bytes,
    mime: str,
    output_base: Path,
    diag_label: str = "",
) -> Any:
    """One Gemini structured-output call. Returns the parsed JSON.

    Caller passes a pre-built `client` and pre-loaded `image_bytes` so
    that multi-call orchestrators (`extract_lodging.py`) build them
    once and reuse across N parallel calls. `image_filename` is inlined
    into the prompt prefix for the model's filename context; `mime` is
    for the inline-data part.

    `output_base` is used only on failure: the raw response text is
    dumped to `<output_base>[.{diag_label}].raw.txt` and structured
    diagnostics to `<output_base>[.{diag_label}].error.txt`. For
    single-call kinds, leave `diag_label=""`. For multi-call kinds,
    `diag_label="main"` / `"extras"` keeps the per-call diagnostics
    distinct so you can tell which call truncated.

    Raises `GeminiCallFailed` if the response can't be parsed as JSON
    (typically because the model truncated mid-output). Returns
    whatever `json.loads` produces — almost always a list, since our
    response schemas are `type: array`.
    """
    from google.genai import types

    response_schema = _load_response_schema(response_schema_path)

    # Stage 21: wrap the generate_content call in retry logic to survive
    # transient Vertex flakiness (rate limit / 5xx / network blip). The
    # JSON-decode path BELOW the call is not retried — truncation +
    # malformed output are model issues, not transient.
    def _do_call():
        return client.models.generate_content(
            model=model,
            contents=[
                f"Filename: {image_filename}\n\n{prompt}",
                types.Part.from_bytes(data=image_bytes, mime_type=mime),
            ],
            config=types.GenerateContentConfig(
                response_mime_type="application/json",
                response_schema=response_schema,
                # Gemini 3 includes "thinking" tokens in this budget. Tamarine
                # spent 7860 thinking tokens at 8192 = too small. After Stage 6
                # added confidence_reason on every leaf the output volume grew
                # again — uber1.pdf blew past 32768 mid-JSON. 65536 leaves
                # headroom for multi-page PDFs (Uber receipts are 2 pages,
                # hotel folios can be longer).
                max_output_tokens=DEFAULT_MAX_OUTPUT_TOKENS,
                temperature=0.0,
            ),
        )

    response = _retry_with_backoff(
        _do_call,
        label=f"generate_content {image_filename}{f' ({diag_label})' if diag_label else ''}",
    )

    raw_text = response.text or ""

    try:
        return json.loads(raw_text)
    except json.JSONDecodeError as err:
        suffix = f".{diag_label}" if diag_label else ""
        raw_path = output_base.with_suffix(output_base.suffix + f"{suffix}.raw.txt")
        err_path = output_base.with_suffix(output_base.suffix + f"{suffix}.error.txt")
        output_base.parent.mkdir(parents=True, exist_ok=True)
        raw_path.write_text(raw_text)
        diag = {
            "json_decode_error": str(err),
            "raw_text_length": len(raw_text),
            "candidates": [
                {
                    "finish_reason": str(getattr(c, "finish_reason", None)),
                    "safety_ratings": [
                        str(r) for r in (getattr(c, "safety_ratings", None) or [])
                    ],
                    "content_parts": len(
                        (getattr(c, "content", None) and c.content.parts) or []
                    ),
                }
                for c in (getattr(response, "candidates", None) or [])
            ],
            "prompt_feedback": str(getattr(response, "prompt_feedback", None)),
            "usage_metadata": str(getattr(response, "usage_metadata", None)),
        }
        err_path.write_text(json.dumps(diag, indent=2))
        label = diag_label or "single call"
        raise GeminiCallFailed(
            f"FAILED to parse JSON ({label}). Raw -> {raw_path}, diag -> {err_path}"
        ) from err


def run_extraction(
    args: argparse.Namespace,
    prompt: str,
    response_schema_path: Path,
) -> int:
    """One Gemini call → structured JSON → disk.

    Used by single-call kinds (meal, transport). Multi-call kinds
    (lodging) orchestrate `single_call` themselves.

    `args` comes from `parse_args()`. `prompt` is the kind-specific
    instructions; `response_schema_path` points to
    `generated/response_schema_<kind>.json`. Returns the process exit
    code (0 on success, non-zero on failure with diagnostics written
    to disk for triage).
    """
    if not args.image.exists():
        sys.exit(f"image not found: {args.image}")

    if not args.project:
        sys.exit(
            "project required: pass --project or set $VERTEX_PROJECT_ID. "
            "Cloud Run gets this from --set-env-vars in deploy/cloudbuild.yaml."
        )

    from google import genai

    # ADC: works on Cloud Run via the metadata server, and locally after
    # `gcloud auth application-default login`. The SDK picks credentials
    # up automatically when none are passed.
    client = genai.Client(
        vertexai=True, project=args.project, location=args.location
    )

    image_bytes = args.image.read_bytes()
    mime = detect_mime_type(args.image)

    # Leapfrog L.3: call Document AI BEFORE Gemini so we can inline the
    # numbered token list into the prompt. Gemini cites token_ids; we
    # resolve those (instead of doing post-hoc text matching) for
    # geometrically-exact bbox grounding. Failure here is non-fatal —
    # `doc is None` just means we feed Gemini the same prompt it always
    # got, and bbox population falls back to the text-matching path.
    doc = None
    try:
        doc = ocr_document(args.image)
    except Exception as err:
        print(
            f"warning: Document AI OCR failed for {args.image}: {err}; "
            "extraction will proceed without token-id grounding.",
            file=sys.stderr,
        )

    prompt_for_gemini = prompt
    if doc is not None:
        token_list_text, _, _ = format_tokens_for_prompt(doc)
        prompt_for_gemini = (
            prompt
            + "\n\n# Numbered Document AI tokens (for `token_ids` grounding)\n\n"
            + token_list_text
        )

    try:
        parsed = single_call(
            client=client,
            model=args.model,
            prompt=prompt_for_gemini,
            response_schema_path=response_schema_path,
            image_filename=args.image.name,
            image_bytes=image_bytes,
            mime=mime,
            output_base=args.output,
        )
    except GeminiCallFailed as err:
        print(err, file=sys.stderr)
        return 1

    args.output.parent.mkdir(parents=True, exist_ok=True)

    # Inject source_filename + populate bboxes. populate_bboxes is
    # dual-path (Leapfrog L.2): token_id resolution when the evidence
    # entry carries token_ids (set by Gemini per the augmented prompt),
    # text-matching fallback otherwise. We pass the already-OCR'd `doc`
    # so we don't pay for a second DocAI call per receipt.
    if isinstance(parsed, list):
        for entry in parsed:
            if isinstance(entry, dict):
                entry["source_filename"] = args.image.name
                populate_bboxes(entry, args.image, doc=doc)
    elif isinstance(parsed, dict):
        parsed["source_filename"] = args.image.name
        populate_bboxes(parsed, args.image, doc=doc)

    args.output.write_text(json.dumps(parsed, indent=2, ensure_ascii=False))
    log_event("extract.wrote", output_path=str(args.output))
    return 0
