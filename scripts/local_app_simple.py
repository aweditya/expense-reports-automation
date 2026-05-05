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

import os
import re
import subprocess
import sys
import uuid
from datetime import datetime
from html import escape as html_escape
from pathlib import Path

from flask import Flask, abort, redirect, request, send_from_directory


REPO_ROOT = Path(__file__).resolve().parent.parent
PYTHON = Path(sys.executable)
UPLOADS_ROOT = REPO_ROOT / ".scratch" / "uploads"

# Per-kind extractor scripts. The upload form's per-file dropdown maps
# directly to the keys here; the dispatcher (extract_all) routes each
# file to the matching script.
EXTRACTORS: dict[str, Path] = {
    "meal": REPO_ROOT / "scripts" / "extract_meal.py",
    "transport": REPO_ROOT / "scripts" / "extract_transport.py",
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

    upload_id = generate_upload_id()
    upload_dir = UPLOADS_ROOT / upload_id
    files_dir = upload_dir / "files"
    extractions_dir = upload_dir / "extractions"
    reduced_path = upload_dir / "reduced" / "report.json"
    workbench_path = upload_dir / "workbench.html"

    files_dir.mkdir(parents=True, exist_ok=True)
    extractions_dir.mkdir(parents=True, exist_ok=True)
    reduced_path.parent.mkdir(parents=True, exist_ok=True)

    saved = save_uploaded_files(pairs, files_dir)
    extract_all(saved, extractions_dir)
    reduce(extractions_dir, reduced_path)
    render_workbench(reduced_path, extractions_dir, workbench_path)

    # POST/Redirect/GET: send the browser to a bookmarkable URL for the
    # rendered workbench. Refresh-friendly; back-button-friendly; no
    # double-submit on reload.
    return redirect(f"/uploads/{upload_id}/workbench.html", code=303)


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

def save_uploaded_files(pairs, dest_dir: Path) -> list[tuple[Path, str]]:
    """Save each uploaded file to dest_dir under a deduped, sanitized name.
    Threads each file's kind through unchanged — the dispatcher (extract_all)
    needs it to pick the right extractor."""
    seen: set[str] = set()
    out: list[tuple[Path, str]] = []
    for f, kind in pairs:
        name = dedupe(sanitize_filename(f.filename), seen)
        path = dest_dir / name
        f.save(str(path))
        out.append((path, kind))
    return out


def extract_all(
    saved: list[tuple[Path, str]], extractions_dir: Path
) -> list[Path]:
    """Phase 1: sequential per-file extraction. Each file routes to the
    extractor matching its FA-supplied kind (meal, transport, …).
    To parallelize later: replace this body with a ThreadPoolExecutor
    over the same call. Nothing downstream cares — the contract is
    list[(input path, kind)] -> list[output JSON paths].
    """
    out: list[Path] = []
    for src, kind in saved:
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
        out.append(out_path)
    return out


def reduce(extractions_dir: Path, reduced_path: Path) -> None:
    run_subprocess(
        rust_bin("reduce_extractions") + [
            "--in", str(extractions_dir),
            "--out", str(reduced_path),
        ],
        label="reduce",
    )


def render_workbench(
    reduced_path: Path,
    extractions_dir: Path,
    workbench_path: Path,
) -> None:
    run_subprocess(
        rust_bin("render_workbench_from_report") + [
            "--report", str(reduced_path),
            "--receipts-dir", str(extractions_dir),
            "--out", str(workbench_path),
        ],
        label="render",
    )


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
  .shell { max-width:600px; margin:80px auto; padding:32px 28px; background:#fff;
           border:1px solid #e5e7eb; border-radius:8px; }
  .eyebrow { margin:0 0 4px; font-size:11px; font-weight:600; text-transform:uppercase;
             letter-spacing:.08em; color:#6b7280; }
  h1 { margin:0 0 16px; font-size:24px; font-weight:600; }
  p  { margin:0 0 16px; color:#4b5563; font-size:14px; line-height:1.5; }
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
</style>
</head>
<body>
<div class="shell">
  <p class="eyebrow">Stanford Expense Report</p>
  <h1>Upload Receipts</h1>
  <p>Pick each receipt (PDF, JPEG, PNG) and tell the system what kind of
     document it is. We'll extract the fields, reduce them into a single
     report, and show you what's filled and what still needs your input.</p>
  <form method="post" action="/upload" enctype="multipart/form-data">
    <div id="file-rows">
      <div class="file-row">
        <input type="file" name="file_0" required
               accept="image/png,image/jpeg,application/pdf">
        <select name="kind_0" required>
          <option value="meal">Meal Receipt</option>
          <option value="transport">Ground Transport</option>
        </select>
      </div>
    </div>
    <div class="actions">
      <button type="button" class="secondary" onclick="addFileRow()">+ Add another file</button>
      <button type="submit">Process Receipts</button>
    </div>
  </form>
  <p class="note">Please don't refresh the page while processing.</p>
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
      </select>
    `;
    document.getElementById('file-rows').appendChild(row);
    rowCount++;
  }
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

if __name__ == "__main__":
    UPLOADS_ROOT.mkdir(parents=True, exist_ok=True)
    print(f"local_app_simple listening on http://{HOST}:{PORT}", flush=True)
    print(f"  uploads dir:         {UPLOADS_ROOT}", flush=True)
    app.run(host=HOST, port=PORT, debug=False)
