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
    "miscellaneous": REPO_ROOT / "scripts" / "extract_miscellaneous.py",
    "membership": REPO_ROOT / "scripts" / "extract_membership.py",
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
    return UPLOAD_FORM_HTML.replace("__FORM_DRAFT_KEY__", FORM_DRAFT_KEY)


# Single source of truth for the browser localStorage key the form-
# persistence JS uses (Stage 11b). Referenced by both the upload form
# (writes drafts on input) and the progress page (clears the draft on
# phase=done after a successful upload). Both templates use the
# `__FORM_DRAFT_KEY__` placeholder which gets string-replaced at render
# time. Bumping the suffix invalidates all existing draft browser-state.
FORM_DRAFT_KEY = "stanford-expense-form-draft-v1"


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
    # Stage 8c.1: SUNet is required by Stanford's foreign-page Airfare row.
    # Form has required + pattern; re-check server-side for non-browser POSTs.
    if not request.form.get("fa_payee_sunet", "").strip():
        return ("Missing required field: Payee SUNet ID. Use the browser's "
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

    # POST-redirect-GET: send the FA to a GET URL for the progress
    # page so a refresh re-fetches state instead of triggering the
    # browser's "Resubmit form?" prompt + a duplicate upload.
    return redirect(f"/upload/status/{upload_id}", code=303)


@app.get("/upload/status/<upload_id>")
def upload_status(upload_id: str):
    """Idempotent GET endpoint for the live-progress page. Refresh-safe:
    a browser reload re-renders the same page and the EventSource on
    the page reconnects to /upload/progress/<id>. Returns 404 if the
    upload directory doesn't exist (typo / stale bookmark) so the FA
    sees a clean error instead of a forever-loading bar."""
    safe_id = sanitize_id(upload_id)
    upload_dir = UPLOADS_ROOT / safe_id
    if not upload_dir.exists():
        abort(404)
    return render_progress_page(safe_id)


@app.get("/upload/progress/<upload_id>")
def upload_progress(upload_id: str):
    """Server-Sent Events stream of JOBS[upload_id] updates. The
    progress page's EventSource subscribes here and gets phase + file
    statuses pushed every 0.5s. Closes once phase reaches 'done',
    'error', or 'not_found' so the browser doesn't hang forever."""
    safe_id = sanitize_id(upload_id)
    upload_dir = UPLOADS_ROOT / safe_id

    def event_stream():
        # Cap the "initializing" grace period so a stale ID (container
        # restart wiped JOBS, or typo'd URL) doesn't stream
        # `{phase: initializing}` forever and hang the EventSource.
        # ~10s is enough for the background thread to set the first
        # JOBS entry on a fresh upload.
        init_ticks_remaining = 20  # 20 × 0.5s = 10s grace
        while True:
            snapshot = _get_job(safe_id)
            if not snapshot:
                if not upload_dir.exists():
                    # No JOBS entry AND no upload directory → truly
                    # unknown. Tell the client + close so they can
                    # show a friendly error page.
                    yield f"data: {json.dumps({'phase': 'not_found'})}\n\n"
                    return
                init_ticks_remaining -= 1
                if init_ticks_remaining <= 0:
                    # Upload dir exists but JOBS never appeared after
                    # 10s — pipeline thread crashed before its first
                    # _set_job, or container restarted and lost JOBS.
                    yield f"data: {json.dumps({'phase': 'lost'})}\n\n"
                    return
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
    return (PROGRESS_PAGE_HTML
            .replace("__UPLOAD_ID__", html_escape(upload_id))
            .replace("__FORM_DRAFT_KEY__", FORM_DRAFT_KEY))


@app.errorhandler(PipelineError)
def handle_pipeline_error(err: PipelineError):
    """Render a friendly error page for any failed pipeline step. Hidden
    technical-details panel for the developer (you/me); the FA sees a clean
    message."""
    file_phrase = f" (file: {err.filename})" if err.filename else ""
    detail_excerpt = err.detail[:2000]  # cap so we don't dump megabytes
    # Stage 18d: __PLACEHOLDER__ pattern (same as upload form + progress
    # page) so error.html's CSS doesn't need brace-doubling for str.format
    # escapes.
    page = (ERROR_PAGE_HTML
            .replace("__STEP__", html_escape(err.step))
            .replace("__FILE_PHRASE__", html_escape(file_phrase))
            .replace("__DETAIL__", html_escape(detail_excerpt)))
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
        "fa_payee_sunet": "payee_sunet",
        "fa_payee_affiliation": "payee_affiliation",
        "fa_event_name": "event_name",
        "fa_bp_who": "business_purpose_who",
        "fa_bp_what": "business_purpose_what",
        "fa_bp_where": "business_purpose_where",
        "fa_bp_why": "business_purpose_why",
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


EXTRACT_MAX_PARALLEL = int(os.environ.get("EXTRACT_MAX_PARALLEL", "4"))


def extract_all(
    saved: list[tuple[Path, str]],
    extractions_dir: Path,
    on_file_start: callable | None = None,
    on_file_done: callable | None = None,
    on_file_fail: callable | None = None,
) -> list[Path]:
    """Phase 1: per-file extraction (parallelized via ThreadPoolExecutor).
    Each file routes to the extractor matching its FA-supplied kind
    (meal, transport, …) and is spawned in its own subprocess so they
    can run truly in parallel (subprocess.run releases the GIL during
    its wait + each child is a fresh Python interpreter with its own
    Vertex SDK client).

    **Stage 17 (2026-05-23):** the loop used to be sequential and a
    6-receipt batch took ~12 min wallclock (~121s/receipt). Per the
    Stage 20 investigation, the bottleneck appeared to be the
    sequential loop itself, not Vertex-side queueing. With this
    change, the SAME 6-receipt batch should drop to ~max(per-file
    times) ≈ 2 min — assuming Vertex's per-project quota
    accommodates 4-6 concurrent extraction subprocesses. Cap via
    EXTRACT_MAX_PARALLEL env var (default 4) so we don't overwhelm
    Vertex if a batch grows large.

    **Per-file isolation (FA feedback 2026-05-22):** still applies.
    Each future independently catches PipelineError and reports via
    `on_file_fail`. Other concurrent files keep processing. The
    pipeline only raises out of here if EVERY file failed — partial
    success proceeds to reduce + render with whatever extractions/
    *.json landed.

    `on_file_start(index, name)` fires before each file's subprocess
    runs; `on_file_done(index, name)` after success; `on_file_fail(
    index, name, detail)` after a per-file failure. All optional.
    Callbacks fire from worker threads, so JOBS-mutating callbacks
    must use JOBS_LOCK (the existing ones already do).
    """
    # Pre-resolve extractors so the unknown-kind error path runs in
    # the main thread (it's a logic bug, not a per-file extraction
    # failure — surface it loudly even though we mark it as a
    # per-file failure for consistency with the per-file callback API).
    work: list[tuple[int, Path, str, Path, Path]] = []  # (idx, src, kind, extractor, out_path)
    failures: list[tuple[str, str]] = []  # (filename, error_detail)
    for idx, (src, kind) in enumerate(saved):
        extractor = EXTRACTORS.get(kind)
        if extractor is None:
            detail = f"unknown kind {kind!r}"
            if on_file_fail is not None:
                on_file_fail(idx, src.name, detail)
            failures.append((src.name, detail))
            continue
        out_path = extractions_dir / f"{src.stem}.json"
        work.append((idx, src, kind, extractor, out_path))

    def _extract_one(item: tuple[int, Path, str, Path, Path]) -> tuple[int, Path | None, str | None]:
        """Runs in a worker thread. Returns (idx, out_path, error_detail)
        — error_detail is None on success. Fires the start/done/fail
        callbacks at the right moments."""
        idx, src, kind, extractor, out_path = item
        if on_file_start is not None:
            on_file_start(idx, src.name)
        try:
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
        except PipelineError as err:
            if on_file_fail is not None:
                on_file_fail(idx, src.name, err.detail)
            return (idx, None, err.detail)
        if on_file_done is not None:
            on_file_done(idx, src.name)
        return (idx, out_path, None)

    out: list[Path] = []
    if work:
        # Cap concurrency at min(len(work), EXTRACT_MAX_PARALLEL). For
        # small batches (1-3 files) all run at once. For larger batches
        # we leave headroom for Vertex's per-project quota (60 req/min
        # default; 4 concurrent extractions × ~3 calls each ≈ 12
        # in-flight at peak).
        max_workers = min(len(work), max(1, EXTRACT_MAX_PARALLEL))
        from concurrent.futures import ThreadPoolExecutor
        with ThreadPoolExecutor(max_workers=max_workers,
                                thread_name_prefix="extract") as pool:
            results = list(pool.map(_extract_one, work))
        # Sort results back into the original file order (ThreadPoolExecutor.map
        # preserves order, but be explicit so a future switch to as_completed
        # doesn't silently scramble the output list — reduce.rs's per-line
        # numbering keys off this order).
        for idx, out_path, err in sorted(results, key=lambda r: r[0]):
            if err is None and out_path is not None:
                out.append(out_path)
            elif err is not None:
                # filename is the source name; look it up from the original
                # `saved` list (idx is into `saved`, not `work`).
                src_name = saved[idx][0].name
                failures.append((src_name, err))

    # Only raise if EVERY file failed — partial success proceeds.
    if not out and failures:
        joined = "; ".join(f"{n}: {d[:100]}" for n, d in failures)
        raise PipelineError(
            step="extract",
            detail=f"all {len(failures)} file(s) failed extraction. {joined}",
        )
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


def fx_enrich(reduced_path: Path) -> None:
    """Post-reduce step: overwrite mock exchange_rate + line_amount_usd
    on foreign lines with real Frankfurter rates. Failure-tolerant —
    skips per-line on network/unsupported errors, leaving the reducer's
    mock in place. See scripts/fx_enrich.py for details."""
    run_subprocess(
        [str(PYTHON), str(REPO_ROOT / "scripts" / "fx_enrich.py"),
         "--in", str(reduced_path), "--out", str(reduced_path)],
        label="fx_enrich",
    )


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

    def file_fail(idx: int, _name: str, detail: str) -> None:
        # Per-file failure (Stage 11c): keep processing the rest of the
        # batch but mark this file as failed + carry a short error
        # message so the progress page can surface it per-file.
        with JOBS_LOCK:
            files = JOBS.setdefault(upload_id, {}).setdefault("files", [])
            if idx < len(files):
                files[idx]["status"] = "failed"
                files[idx]["error"] = detail[:200]

    try:
        _set_job(upload_id, phase="extract")
        extract_all(
            saved, extractions_dir,
            on_file_start=file_start,
            on_file_done=file_done,
            on_file_fail=file_fail,
        )
        _set_job(upload_id, phase="reduce")
        reduce(extractions_dir, reduced_path, fa_input_path=fa_input_path)
        _set_job(upload_id, phase="fx")
        fx_enrich(reduced_path)
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


# ─── HTML templates ────────────────────────────────────────────────────────
# Stage 18d (2026-05-22 cleanup): the three template strings used to live
# inline here (~735 LOC of HTML/CSS/JS); moved to templates/{upload_form,
# progress,error}.html and loaded once at module import. No per-request
# I/O cost. Editors get HTML syntax highlighting; diffs only touch
# templates when the templates change.

_TEMPLATES_DIR = REPO_ROOT / "templates"
UPLOAD_FORM_HTML = (_TEMPLATES_DIR / "upload_form.html").read_text(encoding="utf-8")
PROGRESS_PAGE_HTML = (_TEMPLATES_DIR / "progress.html").read_text(encoding="utf-8")
ERROR_PAGE_HTML = (_TEMPLATES_DIR / "error.html").read_text(encoding="utf-8")


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
    # Werkzeug's dev server defaults to single-threaded — all HTTP
    # requests serialize through one thread, which makes SSE + concurrent
    # POSTs serialize artificially in local testing. Cloud Run uses
    # gunicorn `--workers 1 --threads 8`, so prod is multi-threaded;
    # mirror that here so local stress tests reflect prod behavior.
    app.run(host=HOST, port=PORT, debug=False, threaded=True)
