"""Structured JSON log helper (Stage 22).

Cloud Logging on GCP auto-parses JSON from stdout into structured
fields — severity, message, timestamp, and any custom payload keys
become indexable. That makes queries like
"all extractions that took > 60s" or "all uploads that hit phase=error"
trivial in the Logs Explorer; with plain print() they're a regex grep
over unstructured strings.

This module owns the structured-emit primitives. Two functions:

  log_event(event, **fields)   → INFO-level structured line to stdout
  log_error(event, **fields)   → ERROR-level structured line to stderr

Each line is one JSON object with reserved keys:

  {
    "severity": "INFO" | "ERROR" | "WARNING",
    "event":    str (dotted identifier, e.g. "extract.done"),
    "timestamp": float (seconds since epoch, monotonic UTC),
    ... custom **fields ...
  }

Cloud Logging's GCP-native format expects `severity` as a top-level
field (camelCase or UPPER both accepted) — we use UPPER. The `message`
field is implicit: GCP renders the whole JSON in the log row, but if
you want a one-line summary, pass `message="..."` as a field.

Backward-compatible: legacy `print(..., flush=True)` calls still work
+ get captured by Cloud Logging as unstructured text. Migrate
incrementally — every replaced print is a future query unlocked.

Operator-facing only. FA-facing strings (error pages, status text)
stay as plain text; this module is for the operator's Logs Explorer.
"""

from __future__ import annotations

import json
import sys
import time


def _emit(severity: str, event: str, fields: dict, stream) -> None:
    payload = {
        "severity": severity,
        "event": event,
        "timestamp": time.time(),
        **fields,
    }
    # default=str so datetime, Path, Exception render via str() instead
    # of crashing the logger. Loss of precision is fine — we just need
    # the value queryable in Cloud Logging, not round-trippable.
    print(json.dumps(payload, default=str), file=stream, flush=True)


def log_event(event: str, **fields) -> None:
    """Emit a structured INFO event to stdout. `event` should be a
    dotted identifier like 'extract.start', 'pipeline.phase', or
    'fx.lookup'. Pass any relevant context as kwargs:

        log_event("extract.done", upload_id=uid, filename=name,
                  kind="meal", duration_ms=1234)
    """
    _emit("INFO", event, fields, sys.stdout)


def log_error(event: str, **fields) -> None:
    """Emit a structured ERROR event to stderr. Use for unexpected
    failures the operator should see in Cloud Logging's error-level
    filter. Routine retries / per-file failures that the FA already
    sees on the workbench go through log_event with severity inferred
    from context — log_error is for "something is broken, page the
    operator" signals only."""
    _emit("ERROR", event, fields, sys.stderr)


def log_warning(event: str, **fields) -> None:
    """Emit a structured WARNING. Mid-tier between info and error —
    transient issues, retries, degraded-mode fallbacks (FX rate
    unavailable → mock, DocAI failed → text-match fallback)."""
    _emit("WARNING", event, fields, sys.stdout)
