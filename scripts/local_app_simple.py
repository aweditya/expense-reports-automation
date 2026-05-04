"""Minimal Flask HTTP server for the redesigned expense-report pipeline.

One POST endpoint that takes uploaded receipts, runs:
  spike_extract per file → reduce_extractions → render_workbench
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
SPIKE_EXTRACT = REPO_ROOT / "scripts" / "spike_extract.py"
UPLOADS_ROOT = REPO_ROOT / ".scratch" / "uploads"

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
    files = request.files.getlist("receipts")
    files = [f for f in files if f.filename]
    if not files:
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

    saved_paths = save_uploaded_files(files, files_dir)
    extract_all(saved_paths, extractions_dir)
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

def save_uploaded_files(files, dest_dir: Path) -> list[Path]:
    """Save each uploaded file to dest_dir under a deduped, sanitized name."""
    seen: set[str] = set()
    out: list[Path] = []
    for f in files:
        name = dedupe(sanitize_filename(f.filename), seen)
        path = dest_dir / name
        f.save(str(path))
        out.append(path)
    return out


def extract_all(saved_paths: list[Path], extractions_dir: Path) -> list[Path]:
    """Phase 1: sequential per-file extraction.
    To parallelize later: replace this body with a ThreadPoolExecutor over
    the same call. Nothing downstream cares — the contract is
    list[input paths] -> list[output JSON paths].
    """
    out: list[Path] = []
    for src in saved_paths:
        out_path = extractions_dir / f"{src.stem}.json"
        run_subprocess(
            [
                str(PYTHON),
                str(SPIKE_EXTRACT),
                "--image", str(src),
                "--output", str(out_path),
            ],
            label=f"extract {src.name}",
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
  .shell { max-width:560px; margin:80px auto; padding:32px 28px; background:#fff;
           border:1px solid #e5e7eb; border-radius:8px; }
  .eyebrow { margin:0 0 4px; font-size:11px; font-weight:600; text-transform:uppercase;
             letter-spacing:.08em; color:#6b7280; }
  h1 { margin:0 0 16px; font-size:24px; font-weight:600; }
  p  { margin:0 0 16px; color:#4b5563; font-size:14px; line-height:1.5; }
  input[type=file] { display:block; margin:16px 0 24px; }
  button { background:#1a1d1f; color:#fff; padding:10px 20px; border:0; border-radius:6px;
           font-size:14px; font-weight:500; cursor:pointer; }
  button:hover { background:#374151; }
  .note { font-size:12px; color:#6b7280; margin-top:16px; }
</style>
</head>
<body>
<div class="shell">
  <p class="eyebrow">Stanford Expense Report</p>
  <h1>Upload Receipts</h1>
  <p>Drop in receipts (PDF, JPEG, PNG). The system extracts the fields,
     reduces them into a single report, and shows you what's filled and
     what still needs your input.</p>
  <form method="post" action="/upload" enctype="multipart/form-data">
    <input type="file" name="receipts" multiple
           accept="image/png,image/jpeg,application/pdf">
    <button type="submit">Process Receipts</button>
  </form>
  <p class="note">Please don't refresh the page while processing.</p>
</div>
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
