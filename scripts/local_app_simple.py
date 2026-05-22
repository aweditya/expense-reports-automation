"""Minimal Flask HTTP server for the redesigned expense-report pipeline.

One POST endpoint that takes uploaded receipts (each tagged with a
kind dropdown the FA picks alongside the file), runs:
  extract_<kind> per file → reduce_extractions → render_workbench
and returns the rendered HTML synchronously. No async jobs, no session
state, no editable inputs (those come post-M7).

Designed to run identically locally and on Cloud Run:
  - Reads HOST / PORT from env vars (Cloud Run sets PORT).
  - Per-upload directories live under .scratch/uploads/<id>/ locally;
    Cloud Run uses ephemeral container-local storage.
  - GCP auth: Application Default Credentials. On Cloud Run this is the
    runtime service account via the metadata server. Locally, run once:
    `gcloud auth application-default login`.

Run locally:
  VERTEX_PROJECT_ID=soe-agile-agents ./.venv/bin/python scripts/local_app_simple.py
"""

from __future__ import annotations

import io
import json
import os
import re
import subprocess
import sys
import threading
import time
import uuid
from datetime import datetime
from html import escape as html_escape
from pathlib import Path

from flask import Flask, Response, abort, redirect, request, send_from_directory
from PIL import Image
from pillow_heif import register_heif_opener

# Register the HEIF opener once at module load. After this, Pillow's
# Image.open() transparently handles HEIC/HEIF input like any other format.
# Idempotent — safe to call multiple times.
register_heif_opener()


REPO_ROOT = Path(__file__).resolve().parent.parent
PYTHON = Path(sys.executable)
UPLOADS_ROOT = REPO_ROOT / ".scratch" / "uploads"

# Per-kind extractor scripts. The upload form's per-file dropdown maps
# directly to the keys here; the dispatcher (extract_all) routes each
# file to the matching script.
EXTRACTORS: dict[str, Path] = {
    "meal": REPO_ROOT / "scripts" / "extract_meal.py",
    "transport": REPO_ROOT / "scripts" / "extract_transport.py",
    "lodging": REPO_ROOT / "scripts" / "extract_lodging.py",
    "airfare": REPO_ROOT / "scripts" / "extract_airfare.py",
}

# Cloud Run sets PORT; locally default 8765 (matches existing app's muscle memory).
HOST = os.environ.get("HOST", "0.0.0.0")
PORT = int(os.environ.get("PORT", "8765"))

# In the container we ship pre-built Rust binaries and set RUST_BIN_DIR to
# their install path. Locally we don't — fall back to `cargo run` so dev
# iteration picks up uncompiled source changes without a manual rebuild.
RUST_BIN_DIR = os.environ.get("RUST_BIN_DIR")


def rust_bin(name: str) -> list[str]:
    if RUST_BIN_DIR:
        return [str(Path(RUST_BIN_DIR) / name)]
    return ["cargo", "run", "--quiet", "--bin", name, "--"]


app = Flask(__name__)


class PipelineError(RuntimeError):
    """Raised when a pipeline step (extract / reduce / render) fails. Carries
    enough structured context for the error page to render a friendly message
    + an optional technical-details panel.

    Defined here (above the route handlers) so the @app.errorhandler decorator
    can reference it at module load time.
    """

    def __init__(self, step: str, detail: str, filename: str | None = None):
        self.step = step
        self.filename = filename
        self.detail = detail
        super().__init__(f"{step} failed: {detail[:200]}")


# ─── Async job state (friday Stage 5) ──────────────────────────────────────
#
# JOBS holds per-upload progress so the SSE endpoint can stream it to the
# browser while the extract → reduce → render pipeline runs in a worker
# thread. Single-process (gunicorn --workers 1 + Cloud Run --max-instances=1)
# so a plain dict + lock is fine. Cleared on instance restart; entries are
# small enough that we don't bother garbage-collecting completed jobs
# within the lifetime of an instance.
#
# Shape per upload_id:
#   {
#     "phase":    "extract" | "reduce" | "render" | "done" | "error",
#     "files":    [{"name": str, "kind": str, "status": "pending"|"extracting"|"done"}],
#     "current":  int,   # index into files of the currently-extracting file
#     "error":    str,   # populated only when phase == "error"
#   }
JOBS: dict[str, dict] = {}
JOBS_LOCK = threading.Lock()


def _set_job(upload_id: str, **updates) -> None:
    """Thread-safe partial update of a JOBS entry. Creates the entry if
    it doesn't exist (defensive — callers should init first)."""
    with JOBS_LOCK:
        if upload_id not in JOBS:
            JOBS[upload_id] = {}
        JOBS[upload_id].update(updates)


def _get_job(upload_id: str) -> dict:
    """Thread-safe snapshot read. Returns a shallow copy so the SSE
    serializer can JSON-encode without holding the lock."""
    with JOBS_LOCK:
        return dict(JOBS.get(upload_id, {}))


# ─── Routes ────────────────────────────────────────────────────────────────

@app.get("/")
def upload_form() -> str:
    """Tiny upload form. Inline-styled — no external CSS dependency."""
    return UPLOAD_FORM_HTML


@app.post("/upload")
def upload():
    # Form fields are indexed: file_0 / kind_0, file_1 / kind_1, ...
    # The form JS appends a new (file, kind) pair every time the FA
    # clicks "+ Add another file."
    pairs: list[tuple] = []  # (FileStorage, kind_str)
    for key in sorted(request.files):
        if not key.startswith("file_"):
            continue
        idx = key[len("file_"):]
        f = request.files[key]
        if not f or not f.filename:
            continue
        kind = request.form.get(f"kind_{idx}", "").strip()
        if not kind:
            return (f"Missing kind for file {f.filename!r}.", 400)
        if kind not in EXTRACTORS:
            return (f"Unknown kind {kind!r} for file {f.filename!r} "
                    f"(known: {sorted(EXTRACTORS)}).", 400)
        pairs.append((f, kind))

    if not pairs:
        return ("No files uploaded.", 400)

    # Stage 9a: event_name is FA-compulsory (form has `required`, but a
    # programmatic POST or a JS-disabled browser can bypass that — so
    # re-check server-side). Validator also fires MissingRequiredField
    # if it somehow lands in the report blank, as a third defense.
    if not request.form.get("fa_event_name", "").strip():
        return ("Missing required field: Event name. Use the browser's "
                "back button to return to the form.", 400)

    upload_id = generate_upload_id()
    upload_dir = UPLOADS_ROOT / upload_id
    files_dir = upload_dir / "files"
    extractions_dir = upload_dir / "extractions"
    reduced_path = upload_dir / "reduced" / "report.json"
    workbench_path = upload_dir / "workbench.html"
    fa_input_path = upload_dir / "fa_input.json"

    files_dir.mkdir(parents=True, exist_ok=True)
    extractions_dir.mkdir(parents=True, exist_ok=True)
    reduced_path.parent.mkdir(parents=True, exist_ok=True)

    write_fa_input(request.form, fa_input_path)

    saved = save_uploaded_files(pairs, files_dir)

    # Stage 5 async: initialize the per-upload JOBS entry with the
    # file list, spawn a background thread to run extract → reduce →
    # render, and return the progress page immediately. The page's
    # EventSource subscribes to /upload/progress/<id> and the FA sees
    # phase + per-file status update live. On 'done' it auto-redirects
    # to the workbench (1s delay so the chime registers).
    _set_job(
        upload_id,
        phase="initializing",
        current=0,
        files=[
            {"name": s.name, "kind": k, "status": "pending"}
            for s, k in saved
        ],
        error="",
    )
    threading.Thread(
        target=_run_pipeline_in_background,
        args=(upload_id, saved, extractions_dir, reduced_path,
              workbench_path, fa_input_path),
        name=f"pipeline-{upload_id}",
        daemon=True,
    ).start()

    return render_progress_page(upload_id)


@app.get("/upload/progress/<upload_id>")
def upload_progress(upload_id: str):
    """Server-Sent Events stream of JOBS[upload_id] updates. The
    progress page's EventSource subscribes here and gets phase + file
    statuses pushed every 0.5s. Closes once phase reaches 'done' or
    'error'."""
    safe_id = sanitize_id(upload_id)

    def event_stream():
        while True:
            snapshot = _get_job(safe_id)
            if not snapshot:
                # JOBS hasn't been initialized yet (race) or the upload
                # doesn't exist. Send a minimal placeholder and let the
                # client decide what to do.
                snapshot = {"phase": "initializing"}
            yield f"data: {json.dumps(snapshot)}\n\n"
            if snapshot.get("phase") in ("done", "error"):
                return
            time.sleep(0.5)

    return Response(
        event_stream(),
        mimetype="text/event-stream",
        headers={
            # Disable any intermediate buffering — Cloud Run + gunicorn
            # need this to actually stream rather than buffering the
            # whole response.
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
        },
    )


def render_progress_page(upload_id: str) -> str:
    """The HTML page the FA sees while extraction runs in the
    background. EventSource subscribes to /upload/progress/<id>,
    updates the progress bar + per-file list live, plays a Web Audio
    'done' chime when complete, redirects to the workbench after a
    1-second pause so the chime registers."""
    return PROGRESS_PAGE_HTML.replace("__UPLOAD_ID__", html_escape(upload_id))


@app.errorhandler(PipelineError)
def handle_pipeline_error(err: PipelineError):
    """Render a friendly error page for any failed pipeline step. Hidden
    technical-details panel for the developer (you/me); the FA sees a clean
    message."""
    file_phrase = f" (file: {err.filename})" if err.filename else ""
    detail_excerpt = err.detail[:2000]  # cap so we don't dump megabytes
    page = ERROR_PAGE_HTML.format(
        step=html_escape(err.step),
        file_phrase=html_escape(file_phrase),
        detail=html_escape(detail_excerpt),
    )
    return page, 500


_places_session = None


def _places_session_or_none():
    """Lazy-init an OAuth-bearing session against Google Places API
    (New), reusing the same ADC the extractors use for Vertex. None
    return signals the autocomplete endpoint should return empty so
    the JS gracefully falls back to free-form typing — Places being
    down is not worth taking the form down for."""
    global _places_session
    if _places_session is not None:
        return _places_session
    try:
        from google.auth import default as auth_default
        from google.auth.transport.requests import AuthorizedSession
        credentials, _ = auth_default(
            scopes=["https://www.googleapis.com/auth/cloud-platform"]
        )
        _places_session = AuthorizedSession(credentials)
    except Exception as err:
        app.logger.warning("places: ADC init failed: %s", err)
        return None
    return _places_session


@app.get("/places/autocomplete")
def places_autocomplete():
    """Proxy for Places API (New) Autocomplete. Called by the JS in
    the upload form as the FA types the 'Where' field. Hits the API
    via the gunicorn service account (Cloud Run) or the dev's ADC
    (local) — no API key to provision/rotate."""
    q = request.args.get("q", "").strip()
    if len(q) < 2 or len(q) > 100:
        return ({"suggestions": []}, 200)
    session = _places_session_or_none()
    if session is None:
        return ({"suggestions": []}, 200)
    project = os.environ.get("VERTEX_PROJECT_ID", "")
    headers = {
        "Content-Type": "application/json",
        # Field mask is required by Places API (New) — without it the
        # request errors with FIELD_MASK_REQUIRED. We only need the
        # display text + placeId.
        "X-Goog-FieldMask": "suggestions.placePrediction.text,suggestions.placePrediction.placeId",
    }
    if project:
        # ADC-based requests need an explicit billing project when the
        # creds aren't already attached to one (local dev case).
        headers["X-Goog-User-Project"] = project
    try:
        resp = session.post(
            "https://places.googleapis.com/v1/places:autocomplete",
            json={
                "input": q,
                # Cities, states/provinces, countries — what a FA
                # typing "Where" cares about. Excludes restaurants /
                # addresses / business POIs.
                "includedPrimaryTypes": [
                    "locality",
                    "administrative_area_level_1",
                    "country",
                ],
                "languageCode": "en",
            },
            headers=headers,
            timeout=5,
        )
    except Exception as err:
        app.logger.warning("places: request failed: %s", err)
        return ({"suggestions": []}, 200)
    if not resp.ok:
        app.logger.warning("places: API %s: %s", resp.status_code, resp.text[:300])
        return ({"suggestions": []}, 200)
    raw = resp.json().get("suggestions", [])
    suggestions = []
    for s in raw:
        pred = s.get("placePrediction") or {}
        text = (pred.get("text") or {}).get("text") or ""
        place_id = pred.get("placeId") or ""
        if text:
            suggestions.append({"description": text, "place_id": place_id})
    return ({"suggestions": suggestions}, 200)


_EDIT_PATH_RE = re.compile(r"\[(\d+)\]|\.|([^.\[\]]+)")


def _fa_edit_meta() -> dict:
    """FieldMetadata for a Wrapped<T> that was just created or
    mutated by the FA's edit. Mirrors the shape Rust's FieldMetadata
    serializes (confidence + evidence with kind=user_input + the
    origin string). Used when the walker auto-creates a previously-
    skip-serialized Wrapped, and could also be used to mark in-place
    edits (not done yet — Stage 7 doesn't mark edited cards visually
    in the UI; deferred polish)."""
    return {
        "confidence": "high",
        "confidence_reason": "Edited by FA in the workbench.",
        "evidence": [{"kind": "user_input", "origin": "fa_workbench_edit"}],
        "needs_review": False,
        "flags": [],
    }


def _walk_to_leaf(report: dict, path: str):
    """Walk a dotted-and-bracketed field path
    (e.g. 'expense_report.transaction_lines[0].common.line_amount_usd')
    into the report dict and return (existing_value, setter_fn) where
    setter_fn(coerced_value) mutates the right slot. None on path miss.

    Two leaf shapes:
    1. **Wrapped<T>** (most fields): leaf is `{value, _meta}`. setter
       mutates `.value`.
    2. **Bare scalar** (e.g. `authorized_by` is `Option<String>`).
       setter re-binds on the parent.

    Returning `existing_value` lets the caller pick a coercion
    (float / bool / str) based on the schema-derived type instead of
    a path-name heuristic. Empty-string FA input means "clear" — the
    caller maps it to None when the existing value is None-able.
    """
    # Save the original (full) path for schema-template lookup below;
    # we strip the prefix for traversal but need it for field_types
    # validation when auto-creating.
    original_path = path
    if path.startswith("expense_report."):
        path = path[len("expense_report."):]
    else:
        # Bogus paths (no expense_report prefix) never resolve. The
        # Flask edit endpoint already rejects these with 400; this is
        # defense in depth so the walker is also strict if anyone
        # calls it directly.
        return None
    parts: list[tuple[str, str | int]] = []
    for chunk in path.split("."):
        m = re.match(r"^([^\[]+)((?:\[\d+\])*)$", chunk)
        if not m:
            return None
        parts.append(("key", m.group(1)))
        for idx_match in re.finditer(r"\[(\d+)\]", m.group(2) or ""):
            parts.append(("idx", int(idx_match.group(1))))
    if not parts:
        return None

    parent = None
    last_step: tuple[str, str | int] | None = None
    cursor: object = report
    for i, (kind, val) in enumerate(parts):
        is_last = i == len(parts) - 1
        parent = cursor
        last_step = (kind, val)
        if kind == "key":
            if not isinstance(cursor, dict):
                return None
            if val not in cursor:
                # Last-step miss is benign ONLY if the schema knows
                # about the path — codegen's skip_serializing_if=
                # Wrapped::is_unknown drops empty Wrappeds on
                # serialize, but the renderer still emits a card for
                # them. Auto-create lets the FA's edit land. Bogus
                # paths (not in field_types.json) fail loudly so we
                # don't silently create garbage state.
                if not is_last:
                    return None
                template = _normalize_path_template(original_path)
                if _FIELD_TYPES_BY_PATH and template not in _FIELD_TYPES_BY_PATH:
                    return None
                cursor[val] = {
                    "value": None,
                    "_meta": _fa_edit_meta(),
                }
            cursor = cursor[val]
        else:  # idx
            if not isinstance(cursor, list) or val >= len(cursor):
                return None
            cursor = cursor[val]

    # Wrapped<T> leaf: existing value lives at cursor["value"]; setter
    # mutates it in place. Most fields take this path.
    if isinstance(cursor, dict) and "value" in cursor:
        existing = cursor["value"]
        wrapped = cursor
        def setter(coerced):
            wrapped["value"] = coerced
        return (existing, setter)
    # Bare scalar leaf (e.g. authorized_by is Option<String> directly
    # on the parent dict, not wrapped). Setter re-binds on the parent.
    if last_step is None or parent is None:
        return None
    last_kind, last_val = last_step
    if last_kind == "key" and isinstance(parent, dict):
        existing = cursor
        parent_dict = parent
        key = last_val
        def setter(coerced):
            parent_dict[key] = coerced
        return (existing, setter)
    if last_kind == "idx" and isinstance(parent, list):
        existing = cursor
        parent_list = parent
        idx = last_val
        def setter(coerced):
            parent_list[idx] = coerced
        return (existing, setter)
    return None


def _coerce_to_existing_type(
    raw: str, existing, path: str = ""
) -> tuple[object, str | None]:
    """Coerce FA's string input to the type the schema expects.
    Decision order: existing-value runtime type first (works for
    populated fields), then path-template lookup against the codegen
    field_types.json (works for null-existing fields, which carry no
    runtime type signal).

    Returns (coerced, error_message_or_none).
    """
    s = raw.strip()

    # Booleans first — `bool` is a subclass of `int` in Python, so
    # check it before the int/float branch.
    if isinstance(existing, bool):
        return _coerce_to_bool(s)
    if isinstance(existing, (int, float)):
        return _coerce_to_number(s)
    if isinstance(existing, str) and existing:
        if s == "":
            return (None, None)
        return (s, None)

    # existing is None or empty string — no runtime type signal.
    # Fall back to codegen-emitted field type.
    template = _normalize_path_template(path)
    field_meta = _FIELD_TYPES_BY_PATH.get(template) if template else None
    schema_type = (field_meta or {}).get("type")

    if schema_type == "boolean":
        return _coerce_to_bool(s)
    if schema_type == "number":
        if s == "":
            return (None, None)  # null-clear; render handles Wrapped<f64>=null
        return _coerce_to_number(s)
    if schema_type in ("string", "enum", "date", None):
        if s == "":
            return (None, None)
        return (s, None)
    # Unknown type from schema (array/object slipped through?) — treat as string.
    return (s, None) if s else (None, None)


def _coerce_to_bool(s: str) -> tuple[object, str | None]:
    lowered = s.lower()
    if lowered in ("yes", "true", "1", "y", "t"):
        return (True, None)
    if lowered in ("no", "false", "0", "n", "f"):
        return (False, None)
    if lowered == "":
        return (None, None)  # null-clear
    return (None, "must be yes or no")


def _coerce_to_number(s: str) -> tuple[object, str | None]:
    # Strip $ and , for FA convenience (so "$1,234.56" works).
    cleaned = s.replace("$", "").replace(",", "").strip()
    if cleaned == "":
        return (None, None)  # null-clear
    try:
        return (float(cleaned), None)
    except ValueError:
        return (None, "must be a number (e.g. 32.50)")


# Codegen-emitted single sources of truth — loaded at import time.
# generated/enum_values.json: path_template → list of valid snake_case
#   enum strings.
# generated/field_types.json: path_template → {type, nullable} for
#   every schema leaf. Type values: "string" | "number" | "boolean" |
#   "enum" | "date" (date is a string at the wire layer).
# Keyed by path TEMPLATE — array indices normalized to `[]` so
# `transaction_lines[0].x` and `transaction_lines[3].x` share entries.
_ENUM_VALUES_BY_PATH: dict[str, list[str]] = {}
_FIELD_TYPES_BY_PATH: dict[str, dict] = {}
_GENERATED_DIR = Path(__file__).resolve().parent.parent / "generated"
try:
    _ENUM_VALUES_BY_PATH = dict(
        json.loads((_GENERATED_DIR / "enum_values.json").read_text()).get("enums", {})
    )
except FileNotFoundError:
    pass
try:
    _FIELD_TYPES_BY_PATH = dict(
        json.loads((_GENERATED_DIR / "field_types.json").read_text()).get("fields", {})
    )
except FileNotFoundError:
    # Codegen artifact missing — happens in a fresh clone before
    # `scripts/generate_schema_artifacts.py` has run. Edit endpoint
    # will fall back to runtime-type inference and accept anything
    # for null-existing fields; Rust render catches mistypes at
    # serialize-time with a less friendly error. Not fatal for boot.
    pass


# Match either `[N]` (a numeric index) — gets normalized to `[]` so
# lookups against _ENUM_VALUES_BY_PATH succeed regardless of which
# transaction line / per-diem entry the FA is editing.
_INDEX_RE = re.compile(r"\[\d+\]")


def _normalize_path_template(path: str) -> str:
    """Strip concrete indices so `transaction_lines[0].common.x` and
    `transaction_lines[3].common.x` both look up the same enum
    entry."""
    return _INDEX_RE.sub("[]", path)


def _validate_enum(path: str, value: object) -> str | None:
    """If path's template names an enum field, check value is in the
    valid set. Returns error message or None if OK (or not an enum
    field). Path lookup uses the template (indices normalized to
    `[]`) so lookups match the codegen-emitted JSON."""
    if not isinstance(value, str):
        return None
    template = _normalize_path_template(path)
    valid = _ENUM_VALUES_BY_PATH.get(template)
    if valid is None:
        return None
    if value in valid:
        return None
    # Show up to 5 valid examples so the error msg doesn't get huge.
    preview = ", ".join(valid[:5])
    if len(valid) > 5:
        preview += f", … ({len(valid)} total)"
    return f"must be one of: {preview}"


def _recompute_summary(report: dict) -> None:
    """After an edit, recompute the hero summary fields that derive
    from transaction_lines (total_usd, transaction_date). The render
    binary doesn't recompute these — they're snapshots written by
    the original reducer pass. Keeping the cascade tight: only the
    two summary fields the hero displays. Other derived fields (per-
    line USD if FA edits currency + amount, lodging totals, etc.) are
    NOT cascaded — out of scope for Stage 7 MVP.
    """
    lines = report.get("transaction_lines") or []
    if not lines:
        return
    totals = []
    dates = []
    for line in lines:
        common = (line or {}).get("common") or {}
        usd = (common.get("line_amount_usd") or {}).get("value")
        if isinstance(usd, (int, float)):
            totals.append(float(usd))
        date = (common.get("date") or {}).get("value")
        if isinstance(date, str) and date:
            dates.append(date)
    summary = report.setdefault("transaction_summary", {})
    if totals:
        total_block = summary.setdefault("total_usd", {})
        total_block["value"] = round(sum(totals), 2)
    if dates:
        date_block = summary.setdefault("transaction_date", {})
        date_block["value"] = min(dates)  # ISO date strings sort lexicographically


@app.post("/uploads/<upload_id>/edit")
def edit_field(upload_id: str):
    """Stage 7: FA-side edit-in-place. Frontend POSTs `{path, value}`;
    we walk the path into reduced/report.json, mutate the leaf Wrapped
    object's `value` field, write back, re-render workbench.html so
    the next page load shows the edit.

    JSON body shape: `{"path": "expense_report.transaction_lines[0].common.line_amount_usd", "value": "42.50"}`.
    Returns 200 on success, 400 on bad path/value, 404 if the upload
    doesn't exist.
    """
    safe_id = sanitize_id(upload_id)
    upload_dir = UPLOADS_ROOT / safe_id
    if not upload_dir.is_dir():
        abort(404)
    body = request.get_json(silent=True) or {}
    path = body.get("path", "")
    new_value_raw = body.get("value", "")
    if not isinstance(path, str) or not path.startswith("expense_report"):
        return ({"error": "invalid path"}, 400)
    if not isinstance(new_value_raw, str):
        return ({"error": "value must be a string"}, 400)

    report_path = upload_dir / "reduced" / "report.json"
    if not report_path.exists():
        return ({"error": "report not found"}, 404)
    report = json.loads(report_path.read_text())

    # Walk to leaf first; gives us the existing value (type hint for
    # coercion) + a setter that mutates the right slot.
    walk_result = _walk_to_leaf(report, path)
    if walk_result is None:
        return ({"error": "path did not resolve"}, 400)
    existing, setter = walk_result

    # Coerce FA's raw string to the schema-derived type. Existing
    # value's runtime type is the first signal; path-template lookup
    # against generated/field_types.json is the fallback for null-
    # existing fields where runtime type is unknown.
    coerced, coerce_err = _coerce_to_existing_type(new_value_raw, existing, path)
    if coerce_err is not None:
        return ({"error": coerce_err}, 400)

    # Enum validation: if the path tail names an enum field, reject
    # values outside the known set. Friendly error names a few valid
    # options.
    enum_err = _validate_enum(path, coerced)
    if enum_err is not None:
        return ({"error": enum_err}, 400)

    setter(coerced)

    # Cascade: if the edit touched a transaction line's USD or date,
    # recompute the hero summary so Total USD + Trip Date reflect the
    # new state. Idempotent; safe to call after any edit.
    _recompute_summary(report)

    report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False))

    # Re-render workbench.html (+ both portal CSVs) so the next GET
    # reflects the edit. Re-uses the same render() helper the upload
    # pipeline uses on first processing.
    workbench_path = upload_dir / "workbench.html"
    extractions_dir = upload_dir / "extractions"
    try:
        render_workbench(report_path, extractions_dir, workbench_path)
    except PipelineError as err:
        # Strip filesystem paths from the user-facing message — the FA
        # doesn't need to see /Users/adityasriram/... in their error.
        detail = err.detail
        for prefix in (str(UPLOADS_ROOT), str(REPO_ROOT) if "REPO_ROOT" in globals() else ""):
            if prefix:
                detail = detail.replace(prefix, "…")
        return ({"error": f"render failed: {detail[:200]}"}, 500)
    return ({"ok": True}, 200)


@app.get("/uploads/<upload_id>/<path:filename>")
def serve_upload_file(upload_id: str, filename: str):
    """Static-file route for everything under a per-upload directory:
    workbench.html, extractions/<name>.json, files/<name>, reduced/report.json,
    etc. The renderer emits relative URLs that resolve against this prefix."""
    safe_id = sanitize_id(upload_id)
    upload_dir = UPLOADS_ROOT / safe_id
    if not upload_dir.is_dir():
        abort(404)
    return send_from_directory(upload_dir, filename)


# ─── Pipeline orchestration ────────────────────────────────────────────────

def write_fa_input(form, out_path: Path) -> None:
    """Collect FA-input fields from the upload form and write them as
    `fa_input.json`. Field names match the HTML `name=` attributes
    (`fa_<key>`); the JSON keys match `FaInput`'s field names in
    `src/fa_input.rs` so the Rust side parses them with serde.

    All fields are optional in the JSON. Blank inputs become `None`
    rather than empty strings — keeps the FA's intent ("I didn't fill
    this") distinct from "I typed an empty string." If NO `fa_*` fields
    appear in the form (e.g. a programmatic POST that bypasses the
    fieldset), no file is written and the reducer falls back to the
    no-FA-input flow (S.3 makes --fa-input optional).
    """
    mapping = {
        "fa_payee_name": "payee_name",
        "fa_payee_affiliation": "payee_affiliation",
        "fa_event_name": "event_name",
        "fa_bp_who": "business_purpose_who",
        "fa_bp_what": "business_purpose_what",
        "fa_bp_where": "business_purpose_where",
        "fa_bp_why": "business_purpose_why",
        "fa_bp_key": "business_purpose_key_30char",
        "fa_authorized_by": "authorized_by",
        "fa_rush_processing": "rush_processing",
        "fa_payment_method": "payment_method",
        "fa_foreign_activity_type": "foreign_activity_type",
    }
    data: dict[str, str] = {}
    for form_key, json_key in mapping.items():
        if form_key not in form:
            continue
        value = form.get(form_key, "").strip()
        if value:
            data[json_key] = value
    when_from = form.get("fa_bp_when_from", "").strip()
    when_to = form.get("fa_bp_when_to", "").strip()
    if when_from and when_to and when_from != when_to:
        data["business_purpose_when"] = f"{when_from} to {when_to}"
    elif when_from:
        data["business_purpose_when"] = when_from
    if not data:
        return  # nothing to persist
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(data, indent=2, ensure_ascii=False))


def save_uploaded_files(pairs, dest_dir: Path) -> list[tuple[Path, str]]:
    """Save each uploaded file to dest_dir under a deduped, sanitized name.
    Threads each file's kind through unchanged — the dispatcher (extract_all)
    needs it to pick the right extractor.

    Pre-flight: content-sniff every upload's bytes. If they're HEIC/HEIF
    (iPhone format, often mis-extensioned as .png/.jpg by iOS export), transcode
    to JPEG before writing. Document AI strictly enforces declared MIME vs
    actual bytes and 400s on the mismatch; this normalizes the input so the
    downstream extractors see a real JPEG. See docs/redesign-regrets.md
    2026-05-18 'trusted the file extension; tamarine was HEIC bytes'.
    """
    seen: set[str] = set()
    out: list[tuple[Path, str]] = []
    for f, kind in pairs:
        data = f.read()
        name = sanitize_filename(f.filename)
        converted = convert_heic_to_jpeg_if_needed(data, name)
        if converted is not None:
            data, name = converted
        name = dedupe(name, seen)
        path = dest_dir / name
        path.write_bytes(data)
        out.append((path, kind))
    return out


# HEIF/HEIC brands that may appear at offset 8 inside the ISO-BMFF `ftyp` box.
# Apple iOS writes `heic` (single image) or `heix` (newer); `mif1`/`msf1` are
# the HEIF still-image and sequence brands; `hevc`/`hevx`/`hevm`/`hevs` are
# HEVC-encoded variants; `heim`/`heis` are image-sequence variants. Sniffing
# at the byte level beats trusting the file extension — iOS export commonly
# preserves the source extension while changing the underlying bytes.
_HEIF_BRANDS = (
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis",
    b"hevm", b"hevs", b"mif1", b"msf1",
)


def is_heic_bytes(data: bytes) -> bool:
    """True if the buffer's magic number identifies it as HEIC/HEIF.

    ISO-BMFF layout: bytes 0-3 = box size, bytes 4-7 = 'ftyp', bytes 8-11 =
    major brand. We only need brand-matching; a wider compatibility-brand
    scan would help on edge cases but the common iOS cases hit on major brand.
    """
    if len(data) < 12:
        return False
    if data[4:8] != b"ftyp":
        return False
    return data[8:12] in _HEIF_BRANDS


def convert_heic_to_jpeg_if_needed(
    data: bytes, filename: str
) -> tuple[bytes, str] | None:
    """If `data` is HEIC/HEIF, transcode to JPEG and return (jpeg_bytes,
    new_name_with_jpg_ext). Otherwise return None (caller keeps the original).

    Failure-tolerant: if the transcode itself raises, log a warning and return
    None so the caller falls back to the original bytes. The downstream
    extractors will fail more gracefully than blocking the upload entirely.
    """
    if not is_heic_bytes(data):
        return None
    try:
        with Image.open(io.BytesIO(data)) as img:
            rgb = img.convert("RGB")
            buf = io.BytesIO()
            rgb.save(buf, format="JPEG", quality=95)
            jpeg_bytes = buf.getvalue()
    except Exception as exc:  # noqa: BLE001
        print(
            f"  ! HEIC transcode failed for {filename!r}: {exc}; "
            f"saving original bytes unchanged",
            file=sys.stderr,
            flush=True,
        )
        return None
    new_name = re.sub(r"\.[^.]+$", "", filename) + ".jpg"
    print(
        f"  ▸ transcoded HEIC upload {filename!r} → {new_name!r} "
        f"({len(data)} → {len(jpeg_bytes)} bytes)",
        flush=True,
    )
    return jpeg_bytes, new_name


def extract_all(
    saved: list[tuple[Path, str]],
    extractions_dir: Path,
    on_file_start: callable | None = None,
    on_file_done: callable | None = None,
) -> list[Path]:
    """Phase 1: sequential per-file extraction. Each file routes to the
    extractor matching its FA-supplied kind (meal, transport, …).

    `on_file_start(index, name)` fires before each file's subprocess; the
    background-pipeline runner uses it to update JOBS so the FA's
    progress page sees the file flip to 'extracting'. `on_file_done(
    index, name)` fires after, flipping that file to 'done'. Both are
    optional — synchronous callers pass None and get the original
    behaviour.

    To parallelize later: replace this body with a ThreadPoolExecutor
    over the same call. Nothing downstream cares — the contract is
    list[(input path, kind)] -> list[output JSON paths].
    """
    out: list[Path] = []
    for idx, (src, kind) in enumerate(saved):
        extractor = EXTRACTORS.get(kind)
        if extractor is None:
            # Should be unreachable — /upload validates kind before saving —
            # but raise a PipelineError rather than crash so the FA sees a
            # friendly page instead of a Flask traceback.
            raise PipelineError(
                step="extract",
                detail=f"unknown kind {kind!r} for {src.name}",
                filename=src.name,
            )
        if on_file_start is not None:
            on_file_start(idx, src.name)
        out_path = extractions_dir / f"{src.stem}.json"
        run_subprocess(
            [
                str(PYTHON),
                str(extractor),
                "--image", str(src),
                "--output", str(out_path),
            ],
            label=f"extract {kind} {src.name}",
            filename=src.name,
        )
        if on_file_done is not None:
            on_file_done(idx, src.name)
        out.append(out_path)
    return out


def reduce(
    extractions_dir: Path,
    reduced_path: Path,
    fa_input_path: Path | None = None,
) -> None:
    cmd = rust_bin("reduce_extractions") + [
        "--in", str(extractions_dir),
        "--out", str(reduced_path),
    ]
    if fa_input_path is not None and fa_input_path.exists():
        cmd += ["--fa-input", str(fa_input_path)]
    run_subprocess(cmd, label="reduce")


def render_workbench(
    reduced_path: Path,
    extractions_dir: Path,
    workbench_path: Path,
) -> None:
    # Stanford has two portal pages (domestic + foreign) with different
    # column layouts and Expense Type vocabularies. Emit one CSV per
    # page every time; either may be header-only when no lines route
    # there. The workbench hero hides empty downloads.
    csv_domestic = workbench_path.parent / "lines-domestic.csv"
    csv_foreign = workbench_path.parent / "lines-foreign.csv"
    run_subprocess(
        rust_bin("render_workbench_from_report") + [
            "--report", str(reduced_path),
            "--receipts-dir", str(extractions_dir),
            "--out", str(workbench_path),
            "--csv-domestic-out", str(csv_domestic),
            "--csv-foreign-out", str(csv_foreign),
        ],
        label="render",
    )


def _run_pipeline_in_background(
    upload_id: str,
    saved: list[tuple[Path, str]],
    extractions_dir: Path,
    reduced_path: Path,
    workbench_path: Path,
    fa_input_path: Path,
) -> None:
    """Worker-thread entry point. Runs extract → reduce → render while
    updating JOBS[upload_id] so the SSE endpoint can stream phase +
    per-file progress to the browser. Catches exceptions and writes them
    to JOBS so the progress page can surface a friendly error instead
    of the thread silently dying.
    """
    def file_start(idx: int, _name: str) -> None:
        with JOBS_LOCK:
            job = JOBS.setdefault(upload_id, {})
            files = job.setdefault("files", [])
            if idx < len(files):
                files[idx]["status"] = "extracting"
            job["current"] = idx

    def file_done(idx: int, _name: str) -> None:
        with JOBS_LOCK:
            files = JOBS.setdefault(upload_id, {}).setdefault("files", [])
            if idx < len(files):
                files[idx]["status"] = "done"

    try:
        _set_job(upload_id, phase="extract")
        extract_all(
            saved, extractions_dir,
            on_file_start=file_start,
            on_file_done=file_done,
        )
        _set_job(upload_id, phase="reduce")
        reduce(extractions_dir, reduced_path, fa_input_path=fa_input_path)
        _set_job(upload_id, phase="render")
        render_workbench(reduced_path, extractions_dir, workbench_path)
        _set_job(upload_id, phase="done")
    except PipelineError as err:
        _set_job(upload_id, phase="error",
                 error=f"{err.step}: {err.detail[:300]}")
    except Exception as err:  # noqa: BLE001 — surface anything to the FA
        _set_job(upload_id, phase="error", error=f"unexpected: {err!r}"[:300])


def run_subprocess(cmd: list[str], label: str, filename: str | None = None) -> None:
    """Run a subprocess; raise PipelineError with stderr on failure. Streams
    progress to the server's stdout so an operator can see what's happening."""
    print(f"  ▸ {label} ...", flush=True)
    result = subprocess.run(cmd, capture_output=True, text=True, cwd=str(REPO_ROOT))
    if result.returncode != 0:
        print(f"  ✗ {label} failed (exit {result.returncode})", flush=True)
        print(result.stderr, file=sys.stderr)
        raise PipelineError(step=label, detail=result.stderr.strip(), filename=filename)
    print(f"  ✓ {label} done", flush=True)


# ─── Helpers ───────────────────────────────────────────────────────────────

def generate_upload_id() -> str:
    """e.g. 2026-05-03_15-30-12_abcd1234. Sortable + unique."""
    stamp = datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
    suffix = uuid.uuid4().hex[:8]
    return f"{stamp}_{suffix}"


def sanitize_filename(name: str) -> str:
    name = Path(name).name  # strip any path components
    name = re.sub(r"[^A-Za-z0-9._-]+", "_", name)
    return name or "upload"


def sanitize_id(upload_id: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9_-]+", upload_id):
        abort(400, description="invalid upload id")
    return upload_id


def dedupe(name: str, seen: set[str]) -> str:
    if name not in seen:
        seen.add(name)
        return name
    stem, suffix = Path(name).stem, Path(name).suffix
    counter = 2
    while True:
        candidate = f"{stem}_{counter}{suffix}"
        if candidate not in seen:
            seen.add(candidate)
            return candidate
        counter += 1


# ─── Upload form ───────────────────────────────────────────────────────────

UPLOAD_FORM_HTML = """\
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Stanford Expense Report — Upload</title>
<style>
  body { margin:0; font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;
         background:#f6f7f9; color:#1a1d1f; }
  .shell { max-width:820px; margin:80px auto; padding:32px 28px; background:#fff;
           border:1px solid #e5e7eb; border-radius:8px; }
  .eyebrow { margin:0 0 4px; font-size:11px; font-weight:600; text-transform:uppercase;
             letter-spacing:.08em; color:#6b7280; }
  h1 { margin:0 0 16px; font-size:24px; font-weight:600; }
  h2 { margin:24px 0 12px; font-size:14px; font-weight:600; color:#374151;
       text-transform:uppercase; letter-spacing:.04em; }
  p  { margin:0 0 16px; color:#4b5563; font-size:14px; line-height:1.5; }
  fieldset { border:1px solid #e5e7eb; border-radius:6px; padding:16px 18px 6px;
             margin:0 0 20px; }
  fieldset legend { padding:0 8px; font-size:12px; font-weight:600;
                    text-transform:uppercase; letter-spacing:.06em; color:#6b7280; }
  .field-grid { display:grid; grid-template-columns:1fr 1fr; gap:10px 14px; }
  .field { display:flex; flex-direction:column; gap:3px; margin:0 0 10px; }
  .field.full { grid-column:1 / -1; }
  .field label { font-size:12px; font-weight:500; color:#374151; }
  .field label .opt { font-weight:400; color:#9ca3af; }
  .field label .req { color:#dc2626; margin-left:2px; font-weight:700; }
  .field input[type=text], .field select { padding:6px 8px; border:1px solid #d1d5db;
                                            border-radius:4px; font-size:13px;
                                            background:#fff; color:#1a1d1f; font-family:inherit; }
  .field .hint { font-size:11px; color:#9ca3af; }
  /* Custom combobox dropdown for the "Where" field. Replaces <datalist>
     so the menu inherits our light theme rather than the browser's
     system theme (Safari renders datalist dark in dark mode). */
  .combobox-wrapper { position:relative; }
  .combobox-listbox { position:absolute; top:100%; left:0; right:0; z-index:20;
                      margin:2px 0 0; padding:0; max-height:240px; overflow-y:auto;
                      list-style:none; background:#fff; border:1px solid #d1d5db;
                      border-radius:4px; box-shadow:0 4px 12px rgba(0,0,0,0.08);
                      font-size:13px; color:#1a1d1f; }
  .combobox-option { padding:6px 10px; cursor:pointer; line-height:1.4; }
  .combobox-option:hover,
  .combobox-option.active { background:#e0f2fe; color:#075985; }
  .file-row { display:flex; gap:10px; align-items:center; margin:0 0 12px; }
  .file-row input[type=file] { flex:1; min-width:0; font-size:13px; }
  .file-row select { padding:6px 8px; border:1px solid #d1d5db; border-radius:4px;
                     font-size:13px; background:#fff; color:#1a1d1f; cursor:pointer; }
  .actions { display:flex; gap:12px; align-items:center; margin-top:20px; }
  button { background:#1a1d1f; color:#fff; padding:10px 20px; border:0; border-radius:6px;
           font-size:14px; font-weight:500; cursor:pointer; }
  button:hover { background:#374151; }
  button.secondary { background:#fff; color:#1a1d1f; border:1px solid #d1d5db; }
  button.secondary:hover { background:#f3f4f6; }
  .note { font-size:12px; color:#6b7280; margin-top:16px; }
  /* Friendly callout between the receipts upload and the FA detail
     fieldset (friday Stage 4). Gives the FA confidence about why
     they're being asked to fill more details ('AI will be working
     while you do this') + naturally adds the breathing room
     that the cramped '+ Add another file' → '<fieldset>' transition
     was missing. */
  .ai-callout { background:#eff6ff; border:1px solid #bfdbfe;
                color:#1e40af; font-size:13px; line-height:1.5;
                padding:10px 14px; border-radius:6px;
                margin:14px 0 20px; }
  .ai-callout strong { font-weight:600; }
</style>
</head>
<body>
<div class="shell">
  <p class="eyebrow">Stanford Expense Report</p>
  <h1>Upload Receipts</h1>
  <p>Attach each receipt (PDF, JPEG, PNG) and tell the system what kind of
     document it is, then fill in the report details below. We'll extract
     fields from the receipts, combine them with what you entered, and show
     you what's filled and what still needs your input.</p>
  <form method="post" action="/upload" enctype="multipart/form-data">

    <h2>Receipts</h2>
    <div id="file-rows">
      <div class="file-row">
        <input type="file" name="file_0" required
               accept="image/png,image/jpeg,application/pdf">
        <select name="kind_0" required>
          <option value="meal">Meal Receipt</option>
          <option value="transport">Ground Transport</option>
          <option value="lodging">Lodging Folio</option>
          <option value="airfare">Airfare / Flight Ticket</option>
        </select>
      </div>
    </div>
    <div class="actions">
      <button type="button" class="secondary" onclick="addFileRow()">+ Add another file</button>
    </div>

    <p class="ai-callout">AI extraction takes a couple of minutes once you click
      <strong>Process Receipts</strong> — how about filling in the report
      details below in the meantime?</p>

    <fieldset>
      <legend>Report details</legend>
      <div class="field-grid">
        <div class="field">
          <label for="fa_payee_name">Payee name <span class="req">*</span></label>
          <input type="text" id="fa_payee_name" name="fa_payee_name" required>
        </div>
        <div class="field">
          <label for="fa_payee_affiliation">Payee affiliation <span class="req">*</span></label>
          <select id="fa_payee_affiliation" name="fa_payee_affiliation" required>
            <option value="" disabled selected>Pick one…</option>
            <option value="stanford_faculty">Stanford faculty</option>
            <option value="stanford_staff">Stanford staff</option>
            <option value="stanford_student">Stanford student</option>
            <option value="stanford_postdoc">Stanford postdoc</option>
            <option value="other">Other</option>
          </select>
        </div>
        <div class="field full">
          <label for="fa_event_name">Event name <span class="req">*</span></label>
          <input type="text" id="fa_event_name" name="fa_event_name" required>
          <span class="hint">Conference / event name (e.g. ASPLOS 2026). Use a short label for non-conference trips (e.g. "Field visit — INRIA").</span>
        </div>
        <div class="field">
          <label for="fa_authorized_by">Authorized by <span class="req">*</span></label>
          <input type="text" id="fa_authorized_by" name="fa_authorized_by" required>
          <span class="hint">Approver name / SUNet ID.</span>
        </div>
        <div class="field">
          <label for="fa_rush_processing">Rush processing <span class="req">*</span></label>
          <select id="fa_rush_processing" name="fa_rush_processing" required>
            <option value="no" selected>No</option>
            <option value="yes">Yes</option>
          </select>
        </div>
        <div class="field">
          <label for="fa_payment_method">Payment method <span class="req">*</span></label>
          <input type="text" id="fa_payment_method" name="fa_payment_method" required
                 placeholder="e.g. PCard / Personal">
        </div>
        <div class="field">
          <label for="fa_foreign_activity_type">Foreign activity type <span class="opt">(if applicable)</span></label>
          <select id="fa_foreign_activity_type" name="fa_foreign_activity_type">
            <option value="" selected>Skip if domestic-only</option>
            <option value="conference">Conference</option>
            <option value="research_collaboration">Research collaboration</option>
            <option value="fieldwork">Fieldwork</option>
            <option value="other">Other</option>
          </select>
        </div>
      </div>

      <h2>Business purpose</h2>
      <div class="field-grid">
        <div class="field full">
          <label for="fa_bp_who">Who <span class="req">*</span></label>
          <input type="text" id="fa_bp_who" name="fa_bp_who" required
                 placeholder="e.g. Payee + 2 collaborators">
        </div>
        <div class="field full">
          <label for="fa_bp_what">What <span class="req">*</span></label>
          <input type="text" id="fa_bp_what" name="fa_bp_what" required
                 placeholder="e.g. Presented research at ASPLOS 2026">
        </div>
        <div class="field">
          <label for="fa_bp_when_from">When — from <span class="req">*</span></label>
          <input type="date" id="fa_bp_when_from" name="fa_bp_when_from" required>
        </div>
        <div class="field">
          <label for="fa_bp_when_to">When — to <span class="opt">(single day? leave blank)</span></label>
          <input type="date" id="fa_bp_when_to" name="fa_bp_when_to">
        </div>
        <div class="field">
          <label for="fa_bp_where">Where <span class="req">*</span></label>
          <div class="combobox-wrapper">
            <input type="text" id="fa_bp_where" name="fa_bp_where" required
                   role="combobox" aria-autocomplete="list"
                   aria-expanded="false" aria-controls="where-listbox"
                   autocomplete="off" placeholder="e.g. Pittsburgh, PA">
            <ul id="where-listbox" class="combobox-listbox" role="listbox" hidden></ul>
          </div>
        </div>
        <div class="field full">
          <label for="fa_bp_why">Why <span class="req">*</span></label>
          <input type="text" id="fa_bp_why" name="fa_bp_why" required
                 placeholder="e.g. Advance Stanford research collaboration">
        </div>
        <div class="field full">
          <label for="fa_bp_key">Short label <span class="req">*</span> <span class="opt">(max 30 chars)</span></label>
          <input type="text" id="fa_bp_key" name="fa_bp_key" required maxlength="30"
                 placeholder="e.g. ASPLOS-2026-Pittsburgh">
        </div>
      </div>
    </fieldset>

    <div class="actions">
      <button type="submit">Process Receipts</button>
    </div>
  </form>
  <p class="note">Don't refresh while we process — we'll redirect you when
     it's done.</p>
</div>
<script>
  // Each row is one (file, kind) pair. Indexed names match the server-side
  // parser in /upload (file_0/kind_0, file_1/kind_1, …).
  let rowCount = 1;
  function addFileRow() {
    const row = document.createElement('div');
    row.className = 'file-row';
    row.innerHTML = `
      <input type="file" name="file_${rowCount}" required
             accept="image/png,image/jpeg,application/pdf">
      <select name="kind_${rowCount}" required>
        <option value="meal">Meal Receipt</option>
        <option value="transport">Ground Transport</option>
        <option value="lodging">Lodging Folio</option>
        <option value="airfare">Airfare / Flight Ticket</option>
      </select>
    `;
    document.getElementById('file-rows').appendChild(row);
    rowCount++;
  }

  // Custom combobox for 'Where' autocomplete. Implements the WAI-ARIA
  // combobox 1.2 pattern (role=combobox + role=listbox + role=option
  // + aria-activedescendant). Why custom and not <datalist>: the
  // browser owns datalist rendering and ignores our CSS, so Safari in
  // dark mode shows a dark menu on our light form. This dropdown
  // inherits our theme and behaves consistently across browsers.
  (function setupWhereCombobox() {
    const input = document.getElementById('fa_bp_where');
    const listbox = document.getElementById('where-listbox');
    if (!input || !listbox) return;
    let timer = null;
    let inflight = null;
    let suggestions = [];
    let activeIdx = -1;

    function closeListbox() {
      listbox.hidden = true;
      input.setAttribute('aria-expanded', 'false');
      input.removeAttribute('aria-activedescendant');
      activeIdx = -1;
    }
    function openListbox() {
      if (suggestions.length === 0) { closeListbox(); return; }
      listbox.hidden = false;
      input.setAttribute('aria-expanded', 'true');
    }
    function renderListbox() {
      listbox.innerHTML = suggestions.map(function(s, i) {
        const safe = (s.description || '')
          .replace(/&/g, '&amp;').replace(/"/g, '&quot;')
          .replace(/</g, '&lt;').replace(/>/g, '&gt;');
        return '<li id="where-opt-' + i + '" role="option" ' +
               'class="combobox-option" data-idx="' + i + '">' + safe + '</li>';
      }).join('');
    }
    function setActive(idx) {
      const items = listbox.querySelectorAll('.combobox-option');
      items.forEach(function(el) { el.classList.remove('active'); });
      activeIdx = Math.max(-1, Math.min(idx, suggestions.length - 1));
      if (activeIdx >= 0) {
        const item = listbox.querySelector('#where-opt-' + activeIdx);
        if (item) {
          item.classList.add('active');
          input.setAttribute('aria-activedescendant', item.id);
          item.scrollIntoView({block: 'nearest'});
        }
      }
    }
    function commit(idx) {
      if (idx < 0 || idx >= suggestions.length) return;
      input.value = suggestions[idx].description;
      closeListbox();
    }

    input.addEventListener('input', function() {
      clearTimeout(timer);
      const q = input.value.trim();
      if (q.length < 2) {
        suggestions = [];
        closeListbox();
        return;
      }
      timer = setTimeout(async function() {
        if (inflight) inflight.abort();
        const ctrl = new AbortController();
        inflight = ctrl;
        try {
          const r = await fetch('/places/autocomplete?q=' + encodeURIComponent(q),
                                {signal: ctrl.signal});
          if (!r.ok) return;
          const data = await r.json();
          suggestions = data.suggestions || [];
          renderListbox();
          openListbox();
        } catch (e) { /* abort or net error — silent fallback */ }
      }, 300);
    });

    input.addEventListener('keydown', function(e) {
      if (listbox.hidden && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
        if (suggestions.length > 0) { openListbox(); e.preventDefault(); }
        return;
      }
      if (e.key === 'ArrowDown') { e.preventDefault(); setActive(activeIdx + 1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); setActive(activeIdx - 1); }
      else if (e.key === 'Enter') {
        if (!listbox.hidden && activeIdx >= 0) { e.preventDefault(); commit(activeIdx); }
      }
      else if (e.key === 'Escape') {
        if (!listbox.hidden) { e.preventDefault(); closeListbox(); }
      }
      else if (e.key === 'Tab') {
        // Don't trap focus — just close so the dropdown doesn't linger
        // over whatever field the FA tabs into next.
        closeListbox();
      }
    });

    // mousedown (not click): click fires after blur, which would close
    // the listbox first and eat the selection. mousedown beats blur.
    listbox.addEventListener('mousedown', function(e) {
      const li = e.target.closest('[data-idx]');
      if (!li) return;
      e.preventDefault();
      commit(parseInt(li.dataset.idx, 10));
    });

    document.addEventListener('click', function(e) {
      if (!input.contains(e.target) && !listbox.contains(e.target)) {
        closeListbox();
      }
    });
  })();
</script>
</body>
</html>
"""


# friday Stage 5 — page the FA sees while the background pipeline runs.
# `__UPLOAD_ID__` is replaced by the route handler at render time so the
# JS knows where to subscribe the EventSource. Inline CSS + inline JS,
# same pattern as the other templates in this file — no static-file
# dependency, ships as one string in the gunicorn process.
PROGRESS_PAGE_HTML = """\
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Stanford Expense Report — Processing</title>
<style>
  * { box-sizing: border-box; }
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
         sans-serif; margin:0; padding:0;
         background:#f6f7f9; color:#1a1d1f; }
  .shell { max-width:560px; margin:80px auto; padding:32px 28px; background:#fff;
           border:1px solid #e5e7eb; border-radius:8px;
           box-shadow:0 1px 3px rgba(0,0,0,0.04); }
  .eyebrow { font-size:11px; font-weight:600; text-transform:uppercase;
             letter-spacing:0.08em; color:#6b7280; margin:0 0 8px; }
  h1 { font-size:24px; margin:0 0 6px; }
  .status { font-size:14px; color:#4b5563; margin:0 0 20px; min-height:20px; }
  .bar-outer { width:100%; height:8px; background:#e5e7eb; border-radius:999px;
               overflow:hidden; margin:0 0 24px; }
  .bar-inner { height:100%; background:#2563eb; width:0%;
               transition:width 0.4s ease; }
  .bar-inner.done { background:#10b981; }
  .bar-inner.error { background:#ef4444; }
  .file-list { margin:0 0 8px; padding:0; list-style:none;
               border-top:1px solid #f3f4f6; }
  .file-item { display:flex; align-items:center; gap:10px;
               padding:10px 0; border-bottom:1px solid #f3f4f6;
               font-size:13px; }
  .file-status { width:18px; height:18px; border-radius:50%;
                 display:inline-flex; align-items:center; justify-content:center;
                 font-size:12px; line-height:1; flex-shrink:0;
                 background:#e5e7eb; color:#6b7280; }
  .file-status.extracting { background:#dbeafe; color:#1e40af;
                            animation:pulse 1.2s ease-in-out infinite; }
  .file-status.done { background:#dcfce7; color:#15803d; }
  @keyframes pulse {
    0%, 100% { transform: scale(1); opacity: 1; }
    50% { transform: scale(0.85); opacity: 0.7; }
  }
  .file-name { flex:1; min-width:0; overflow:hidden;
               text-overflow:ellipsis; white-space:nowrap; }
  .file-kind { font-size:11px; color:#9ca3af; }
  .note { font-size:12px; color:#6b7280; margin:16px 0 0; }
  .error-box { background:#fef2f2; border:1px solid #fecaca; color:#991b1b;
               padding:12px 14px; border-radius:6px; margin:0 0 16px;
               font-size:13px; }
  .error-box[hidden] { display:none; }
  .retry-link { color:#1e40af; text-decoration:none; font-weight:500; }
  .retry-link:hover { text-decoration:underline; }
</style>
</head>
<body>
<div class="shell">
  <p class="eyebrow">Stanford Expense Report</p>
  <h1>Processing your receipts</h1>
  <p id="status" class="status">Getting ready…</p>
  <div class="bar-outer">
    <div id="bar" class="bar-inner"></div>
  </div>
  <p id="error-box" class="error-box" hidden></p>
  <ul id="file-list" class="file-list"></ul>
  <p class="note">Don't refresh — we'll redirect you when it's done.</p>
</div>
<script>
  const UPLOAD_ID = "__UPLOAD_ID__";
  const statusEl = document.getElementById('status');
  const barEl = document.getElementById('bar');
  const errEl = document.getElementById('error-box');
  const listEl = document.getElementById('file-list');

  // Render the per-file list once we have the initial JOBS snapshot.
  // Subsequent updates only flip status classes — same DOM nodes.
  let listRendered = false;
  function renderFileList(files) {
    if (listRendered) return;
    listEl.innerHTML = files.map(function(f, i) {
      return '<li class="file-item" data-idx="' + i + '">' +
             '  <span class="file-status pending" data-status>•</span>' +
             '  <span class="file-name">' +
                  f.name.replace(/[<>&]/g, function(c) {
                    return {'<':'&lt;','>':'&gt;','&':'&amp;'}[c];
                  }) +
             '  </span>' +
             '  <span class="file-kind">' + f.kind + '</span>' +
             '</li>';
    }).join('');
    listRendered = true;
  }

  function updateFileStatuses(files) {
    files.forEach(function(f, i) {
      const item = listEl.querySelector('[data-idx="' + i + '"] [data-status]');
      if (!item) return;
      item.className = 'file-status ' + f.status;
      item.textContent = f.status === 'done' ? '✓' :
                         f.status === 'extracting' ? '…' : '•';
    });
  }

  // Map (phase, current, total) → 0–100% for the progress bar.
  // Extract takes the bulk of the time so it gets 0-70%; the
  // reduce + render steps are fast and just bump to 80/90; done = 100.
  function pctFor(snapshot) {
    const phase = snapshot.phase;
    const files = snapshot.files || [];
    const total = files.length || 1;
    const done = files.filter(function(f) { return f.status === 'done'; }).length;
    if (phase === 'initializing') return 2;
    if (phase === 'extract') return 5 + Math.round(65 * done / total);
    if (phase === 'reduce') return 80;
    if (phase === 'render') return 90;
    if (phase === 'done') return 100;
    if (phase === 'error') return 100;
    return 0;
  }

  function statusFor(snapshot) {
    const phase = snapshot.phase;
    const files = snapshot.files || [];
    if (phase === 'initializing') return 'Getting ready…';
    if (phase === 'extract') {
      const cur = (snapshot.current || 0);
      const f = files[cur];
      const name = f ? f.name : '';
      return 'Extracting receipt ' + (cur + 1) + ' of ' + files.length +
             (name ? ': ' + name : '');
    }
    if (phase === 'reduce') return 'Combining extracted data into one report…';
    if (phase === 'render') return 'Rendering your workbench…';
    if (phase === 'done') return 'Done ✓';
    if (phase === 'error') return 'Something went wrong';
    return phase;
  }

  // Soft 2-note "complete" chime via Web Audio API. No file, no CDN.
  // Silently no-ops if the browser blocks audio (no recent user
  // gesture); we don't let that block the redirect.
  function playDoneChime() {
    try {
      const ctx = new (window.AudioContext || window.webkitAudioContext)();
      function tone(freq, startOffset, duration) {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = 'sine';
        osc.frequency.value = freq;
        gain.gain.setValueAtTime(0, ctx.currentTime + startOffset);
        gain.gain.linearRampToValueAtTime(0.15, ctx.currentTime + startOffset + 0.02);
        gain.gain.linearRampToValueAtTime(0, ctx.currentTime + startOffset + duration);
        osc.connect(gain).connect(ctx.destination);
        osc.start(ctx.currentTime + startOffset);
        osc.stop(ctx.currentTime + startOffset + duration);
      }
      tone(523.25, 0, 0.18);    // C5
      tone(659.25, 0.15, 0.25); // E5 overlapping for a chord-y feel
    } catch (e) { /* no audio — that's fine */ }
  }

  const es = new EventSource('/upload/progress/' + UPLOAD_ID);
  let redirected = false;
  es.onmessage = function(e) {
    let snap;
    try { snap = JSON.parse(e.data); } catch (err) { return; }
    if (snap.files) {
      renderFileList(snap.files);
      updateFileStatuses(snap.files);
    }
    statusEl.textContent = statusFor(snap);
    barEl.style.width = pctFor(snap) + '%';
    if (snap.phase === 'done') {
      barEl.classList.add('done');
      if (!redirected) {
        redirected = true;
        playDoneChime();
        es.close();
        // 1s pause so the chime + status text register before redirect.
        setTimeout(function() {
          window.location = '/uploads/' + UPLOAD_ID + '/workbench.html';
        }, 1000);
      }
    } else if (snap.phase === 'error') {
      barEl.classList.add('error');
      errEl.textContent = snap.error || 'Unknown error';
      errEl.innerHTML += ' &nbsp;<a class="retry-link" href="/">Try again →</a>';
      errEl.hidden = false;
      es.close();
    }
  };
  es.onerror = function() {
    // Network blip / SSE got cut. The stream will reconnect on its
    // own — no UI change needed unless we already errored out.
  };
</script>
</body>
</html>
"""


ERROR_PAGE_HTML = """\
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Stanford Expense Report — Error</title>
<style>
  body {{ margin:0; font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;
         background:#f6f7f9; color:#1a1d1f; }}
  .shell {{ max-width:640px; margin:80px auto; padding:32px 28px; background:#fff;
           border:1px solid #e5e7eb; border-radius:8px; }}
  .eyebrow {{ margin:0 0 4px; font-size:11px; font-weight:600; text-transform:uppercase;
             letter-spacing:.08em; color:#6b7280; }}
  h1 {{ margin:0 0 16px; font-size:24px; font-weight:600; }}
  .msg {{ background:#fef2f2; border-left:3px solid #ef4444; padding:12px 14px;
         border-radius:4px; margin:0 0 20px; color:#7f1d1d; font-size:14px; }}
  details {{ margin:20px 0; }}
  summary {{ cursor:pointer; color:#2563eb; font-size:13px; }}
  pre {{ background:#f3f4f6; padding:12px; border-radius:4px; overflow-x:auto;
        font-size:12px; line-height:1.4; color:#374151; max-height:400px;
        overflow-y:auto; }}
  a.try-again {{ display:inline-block; background:#1a1d1f; color:#fff; padding:10px 20px;
                border-radius:6px; font-size:14px; font-weight:500; text-decoration:none; }}
  a.try-again:hover {{ background:#374151; }}
</style>
</head>
<body>
<div class="shell">
  <p class="eyebrow">Stanford Expense Report</p>
  <h1>Something went wrong</h1>
  <p class="msg">The <strong>{step}</strong> step failed{file_phrase}.</p>
  <details>
    <summary>Show technical details</summary>
    <pre>{detail}</pre>
  </details>
  <p><a class="try-again" href="/">Try again</a></p>
</div>
</body>
</html>
"""


# ─── Entry point ───────────────────────────────────────────────────────────

def ensure_vertex_project_env() -> None:
    """On Cloud Run, VERTEX_PROJECT_ID is injected via --set-env-vars in
    deploy/cloudbuild.yaml. Locally, FAs running the dev server have to
    set it themselves — easy to forget, and the extractor's 'project
    required' error doesn't say where to set it. Auto-fill from the
    active gcloud config (the same project the auth checklist points
    at) so local runs Just Work.
    """
    if os.environ.get("VERTEX_PROJECT_ID"):
        return
    try:
        result = subprocess.run(
            ["gcloud", "config", "get-value", "project"],
            capture_output=True, text=True, timeout=5,
        )
        project = (result.stdout or "").strip()
    except (subprocess.SubprocessError, FileNotFoundError):
        project = ""
    if project and project != "(unset)":
        os.environ["VERTEX_PROJECT_ID"] = project
        print(f"  VERTEX_PROJECT_ID:   {project} (from gcloud config)", flush=True)
    else:
        print(
            "  WARNING: VERTEX_PROJECT_ID is unset and no active gcloud project. "
            "Run `gcloud config set project soe-agile-agents` or "
            "`export VERTEX_PROJECT_ID=soe-agile-agents` before uploading.",
            flush=True,
        )


if __name__ == "__main__":
    UPLOADS_ROOT.mkdir(parents=True, exist_ok=True)
    print(f"local_app_simple listening on http://{HOST}:{PORT}", flush=True)
    print(f"  uploads dir:         {UPLOADS_ROOT}", flush=True)
    ensure_vertex_project_env()
    app.run(host=HOST, port=PORT, debug=False)
