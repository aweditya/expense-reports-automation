#!/usr/bin/env python3

import argparse
import html
import http.server
import io
import json
import mimetypes
import os
import shutil
import socketserver
import subprocess
import tempfile
import urllib.parse
from dataclasses import dataclass
from email.parser import BytesParser
from email.policy import default
from pathlib import Path
from typing import Callable


DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 8765
DEFAULT_MODEL = "gemini-3-flash-preview"
DEFAULT_LOCATION = "global"


@dataclass
class UploadedFile:
    filename: str
    content_type: str
    content: bytes


@dataclass
class UploadRequest:
    fields: dict[str, str]
    files: list[UploadedFile]


@dataclass
class BundleListEntry:
    bundle_id: str
    current_stage: str
    updated_at_epoch_ms: int
    latest_run_id: str | None
    document_count: int
    run_count: int


@dataclass
class LocalAppConfig:
    repo_root: Path
    workspace_root: Path
    host: str
    port: int


class LocalAppError(RuntimeError):
    pass


def ensure_safe_bundle_id(bundle_id: str) -> str:
    if not bundle_id:
        raise LocalAppError("bundle identifier is required")
    if sanitize_identifier(bundle_id) != bundle_id:
        raise LocalAppError(f"invalid bundle identifier: {bundle_id}")
    return bundle_id


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a tiny local upload app for the expense report ingestion workspace."
    )
    parser.add_argument(
        "--workspace-root",
        default="/tmp/expense_reports_local_app_workspace",
        help="Directory where bundle workspaces should be stored",
    )
    parser.add_argument(
        "--host",
        default=DEFAULT_HOST,
        help="Host interface to bind; defaults to 127.0.0.1",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=DEFAULT_PORT,
        help="Port to bind; defaults to 8765",
    )
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def list_bundles(workspace_root: Path) -> list[BundleListEntry]:
    bundles_dir = workspace_root / "bundles"
    if not bundles_dir.exists():
        return []

    entries = []
    for manifest_path in sorted(bundles_dir.glob("*/bundle_manifest.json")):
        manifest = json.loads(manifest_path.read_text())
        entries.append(
            BundleListEntry(
                bundle_id=manifest["bundle_id"],
                current_stage=manifest["current_stage"],
                updated_at_epoch_ms=manifest["updated_at_epoch_ms"],
                latest_run_id=manifest.get("latest_run_id"),
                document_count=len(manifest.get("documents") or []),
                run_count=len(manifest.get("runs") or []),
            )
        )

    entries.sort(key=lambda entry: entry.updated_at_epoch_ms, reverse=True)
    return entries


def load_bundle_manifest(workspace_root: Path, bundle_id: str) -> dict:
    bundle_id = ensure_safe_bundle_id(bundle_id)
    manifest_path = workspace_root / "bundles" / bundle_id / "bundle_manifest.json"
    if not manifest_path.exists():
        raise LocalAppError(f"bundle not found: {bundle_id}")
    return json.loads(manifest_path.read_text())


def latest_run_artifacts_dir(workspace_root: Path, bundle_id: str) -> Path | None:
    manifest = load_bundle_manifest(workspace_root, bundle_id)
    latest_run_id = manifest.get("latest_run_id")
    if not latest_run_id:
        return None
    bundle_root = workspace_root / "bundles" / bundle_id
    run = next(
        (run for run in manifest.get("runs") or [] if run["run_id"] == latest_run_id),
        None,
    )
    if not run:
        return None
    return bundle_root / run["output_dir"]


def latest_workbench_path(workspace_root: Path, bundle_id: str) -> Path | None:
    artifacts_dir = latest_run_artifacts_dir(workspace_root, bundle_id)
    if not artifacts_dir:
        return None
    path = artifacts_dir / "review_workbench.html"
    return path if path.exists() else None


def parse_multipart_request(
    content_type: str, body: bytes, encoding: str = "utf-8"
) -> UploadRequest:
    if "multipart/form-data" not in content_type:
        raise LocalAppError("request content type must be multipart/form-data")

    message = BytesParser(policy=default).parsebytes(
        f"Content-Type: {content_type}\r\nMIME-Version: 1.0\r\n\r\n".encode("utf-8")
        + body
    )
    fields: dict[str, str] = {}
    files: list[UploadedFile] = []

    for part in message.iter_parts():
        name = part.get_param("name", header="content-disposition")
        if not name:
            continue
        filename = part.get_filename()
        payload = part.get_payload(decode=True) or b""
        if filename:
            files.append(
                UploadedFile(
                    filename=Path(filename).name,
                    content_type=part.get_content_type(),
                    content=payload,
                )
            )
        else:
            fields[name] = payload.decode(encoding).strip()

    return UploadRequest(fields=fields, files=files)


def save_uploaded_files(temp_dir: Path, files: list[UploadedFile]) -> list[Path]:
    saved_paths = []
    used_names: set[str] = set()
    for upload in files:
        path = temp_dir / dedupe_uploaded_filename(
            sanitize_filename(upload.filename),
            used_names,
        )
        path.write_bytes(upload.content)
        saved_paths.append(path)
    return saved_paths


def dedupe_uploaded_filename(filename: str, used_names: set[str]) -> str:
    candidate = filename
    stem = Path(filename).stem or "upload"
    suffix = Path(filename).suffix
    counter = 2
    while candidate in used_names:
        candidate = f"{stem}_{counter}{suffix}"
        counter += 1
    used_names.add(candidate)
    return candidate


def sanitize_filename(value: str) -> str:
    cleaned = []
    previous_separator = False
    for char in value:
        if char.isalnum() or char in {".", "_", "-"}:
            cleaned.append(char)
            previous_separator = False
        elif not previous_separator:
            cleaned.append("_")
            previous_separator = True
    result = "".join(cleaned).strip("._")
    return result or "upload"


def resolve_cli_command(repo_root: Path) -> list[str]:
    candidate = repo_root / "target" / "debug" / "ingest_bundle_workspace"
    if candidate.exists() and os.access(candidate, os.X_OK):
        return [str(candidate)]
    return ["cargo", "run", "--bin", "ingest_bundle_workspace", "--"]


def build_ingest_command(
    repo_root: Path,
    workspace_root: Path,
    form_fields: dict[str, str],
    input_paths: list[Path],
) -> list[str]:
    bundle_id = sanitize_identifier(form_fields.get("bundle_id") or default_bundle_id(input_paths))
    engine = form_fields.get("engine") or "builtin"
    command = resolve_cli_command(repo_root) + [
        "stage-and-run",
        "--workspace-root",
        str(workspace_root),
        "--bundle-id",
        bundle_id,
        "--fx",
        form_fields.get("fx", "demo"),
        "--engine",
        engine,
    ]
    if form_fields.get("run_id"):
        command.extend(["--run-id", sanitize_identifier(form_fields["run_id"])])
    if form_fields.get("user_id"):
        command.extend(["--user-id", form_fields["user_id"]])
    if engine == "vertex-gemini-sdk":
        command.extend(
            [
                "--location",
                form_fields.get("location", DEFAULT_LOCATION),
                "--model",
                form_fields.get("model", DEFAULT_MODEL),
            ]
        )
        if form_fields.get("project"):
            command.extend(["--project", form_fields["project"]])
        if form_fields.get("service_account_key"):
            command.extend(["--service-account-key", form_fields["service_account_key"]])
        if form_fields.get("sdk_python"):
            command.extend(["--sdk-python", form_fields["sdk_python"]])
    command.extend(str(path) for path in input_paths)
    return command


def run_pipeline_command(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, text=True, capture_output=True)


def handle_upload_submission(
    repo_root: Path,
    workspace_root: Path,
    request: UploadRequest,
    command_runner: Callable[[list[str], Path], subprocess.CompletedProcess[str]] = run_pipeline_command,
) -> str:
    if not request.files:
        raise LocalAppError("at least one document upload is required")

    with tempfile.TemporaryDirectory(prefix="expense_local_app_") as temp_dir_str:
        temp_dir = Path(temp_dir_str)
        input_paths = save_uploaded_files(temp_dir, request.files)
        command = build_ingest_command(repo_root, workspace_root, request.fields, input_paths)
        completed = command_runner(command, repo_root)
        if completed.returncode != 0:
            raise LocalAppError((completed.stderr or completed.stdout).strip() or "pipeline failed")
        return request.fields.get("bundle_id") or default_bundle_id(input_paths)


def render_index_page(config: LocalAppConfig, bundles: list[BundleListEntry], message: str | None = None) -> str:
    items = []
    for bundle in bundles[:30]:
        items.append(
            "<li><a href=\"/bundle/{bundle_id}\">{bundle_id}</a> · {stage} · {documents} docs · {runs} run(s)</li>".format(
                bundle_id=html.escape(bundle.bundle_id),
                stage=html.escape(bundle.current_stage),
                documents=bundle.document_count,
                runs=bundle.run_count,
            )
        )
    bundles_html = "\n".join(items) or "<li>No bundles yet.</li>"

    notice = (
        f"<p class=\"notice\">{html.escape(message)}</p>\n" if message else ""
    )

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Expense Reports Local App</title>
  <style>
    body {{ font-family: Georgia, 'Times New Roman', serif; margin: 0; background: linear-gradient(180deg, #f7f2e8 0%, #efe6d6 100%); color: #1f1a17; }}
    .shell {{ max-width: 1120px; margin: 0 auto; padding: 32px 24px 48px; }}
    .hero {{ display: grid; grid-template-columns: 1.2fr 0.8fr; gap: 24px; align-items: start; }}
    .card {{ background: rgba(255,255,255,0.82); border: 1px solid rgba(65,47,33,0.12); border-radius: 20px; padding: 22px; box-shadow: 0 10px 30px rgba(52,36,24,0.08); }}
    h1, h2 {{ margin: 0 0 12px; }}
    p {{ line-height: 1.5; }}
    .notice {{ background: #f4efe2; border-left: 4px solid #855d34; padding: 12px 14px; border-radius: 8px; }}
    form {{ display: grid; gap: 12px; }}
    label {{ display: grid; gap: 6px; font-weight: 600; }}
    input, select {{ font: inherit; padding: 10px 12px; border-radius: 10px; border: 1px solid #c9bca8; background: white; }}
    input[type=file] {{ padding: 8px; }}
    button {{ font: inherit; background: #2f5b53; color: white; border: 0; border-radius: 999px; padding: 12px 18px; cursor: pointer; }}
    .ghost-button {{ background: transparent; color: #2f5b53; border: 1px solid #9db5b0; }}
    .grid {{ display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }}
    ul {{ margin: 0; padding-left: 18px; }}
    a {{ color: #204c63; }}
    .meta {{ color: #5d5349; font-size: 0.95rem; }}
    .pending-uploads {{ padding: 14px 16px; border: 1px solid #dbcdb7; border-radius: 14px; background: rgba(244, 239, 226, 0.7); }}
    .pending-uploads ul {{ margin-top: 8px; }}
    .pending-uploads li {{ display: flex; justify-content: space-between; gap: 12px; align-items: center; margin-bottom: 8px; }}
    .pending-file-name {{ overflow-wrap: anywhere; }}
    .error-text {{ color: #8d2c21; font-weight: 600; }}
    @media (max-width: 920px) {{ .hero, .grid {{ grid-template-columns: 1fr; }} }}
  </style>
  <script>
    const pendingFiles = [];

    function pendingLabel(file) {{
      return `${{file.name}} (${{file.size}} bytes)`;
    }}

    function renderPendingUploads() {{
      const list = document.getElementById("pending-documents");
      if (!list) {{
        return;
      }}
      if (pendingFiles.length === 0) {{
        list.innerHTML = "<li>No documents selected yet.</li>";
        return;
      }}
      list.innerHTML = "";
      pendingFiles.forEach((file, index) => {{
        const item = document.createElement("li");
        const label = document.createElement("span");
        label.className = "pending-file-name";
        label.textContent = pendingLabel(file);
        const removeButton = document.createElement("button");
        removeButton.type = "button";
        removeButton.className = "ghost-button";
        removeButton.textContent = "Remove";
        removeButton.addEventListener("click", () => {{
          pendingFiles.splice(index, 1);
          renderPendingUploads();
        }});
        item.appendChild(label);
        item.appendChild(removeButton);
        list.appendChild(item);
      }});
    }}

    function addPendingFiles(input) {{
      for (const file of Array.from(input.files || [])) {{
        pendingFiles.push(file);
      }}
      input.value = "";
      clearDocumentError();
      renderPendingUploads();
    }}

    function clearDocumentError() {{
      const error = document.getElementById("document-error");
      if (error) {{
        error.hidden = true;
        error.textContent = "";
      }}
    }}

    function setDocumentError(message) {{
      const error = document.getElementById("document-error");
      if (!error) {{
        return;
      }}
      error.hidden = false;
      error.textContent = message;
    }}

    function syncPendingFilesToInput(input) {{
      if (pendingFiles.length === 0) {{
        return input.files && input.files.length > 0;
      }}
      const transfer = new DataTransfer();
      for (const file of pendingFiles) {{
        transfer.items.add(file);
      }}
      input.files = transfer.files;
      return input.files.length > 0;
    }}

    document.addEventListener("DOMContentLoaded", () => {{
      const form = document.getElementById("upload-form");
      const input = document.getElementById("documents-input");
      if (!form || !input) {{
        return;
      }}
      input.addEventListener("change", () => addPendingFiles(input));
      form.addEventListener("submit", (event) => {{
        if (!syncPendingFilesToInput(input)) {{
          event.preventDefault();
          setDocumentError("Select at least one document before running the pipeline.");
        }}
      }});
      renderPendingUploads();
    }});
  </script>
</head>
<body>
  <div class="shell">
    <section class="hero">
      <div class="card">
        <p class="meta">Expense Reports Automation</p>
        <h1>Local FA Intake App</h1>
        <p>This app uploads documents into the managed bundle workspace, runs the ingestion pipeline, and then opens the generated FA workbench for copy-and-paste filing.</p>
        {notice}
        <form id="upload-form" method="post" action="/upload" enctype="multipart/form-data">
          <div class="grid">
            <label>Bundle ID
              <input name="bundle_id" placeholder="optional_bundle_id">
            </label>
            <label>User ID
              <input name="user_id" placeholder="fa_or_requester_id">
            </label>
            <label>Run ID
              <input name="run_id" placeholder="optional_run_id (leave blank to auto-generate)">
            </label>
            <label>FX Mode
              <select name="fx">
                <option value="demo">demo</option>
                <option value="none">none</option>
              </select>
            </label>
            <label>Engine
              <select name="engine">
                <option value="builtin">builtin</option>
                <option value="vertex-gemini-sdk">vertex-gemini-sdk</option>
              </select>
            </label>
            <label>Project
              <input name="project" placeholder="optional Vertex project">
            </label>
            <label>Location
              <input name="location" value="{html.escape(DEFAULT_LOCATION)}">
            </label>
            <label>Model
              <input name="model" value="{html.escape(DEFAULT_MODEL)}">
            </label>
            <label>Service Account Key
              <input name="service_account_key" placeholder="/abs/path/to/service-account.json">
            </label>
            <label>SDK Python
              <input name="sdk_python" placeholder="/tmp/expense_report_genai_venv/bin/python">
            </label>
          </div>
          <label>Documents
            <input id="documents-input" type="file" name="documents" multiple>
          </label>
          <div class="pending-uploads">
            <p class="meta">Pending uploads. You can reopen the file picker and selections will accumulate until you submit.</p>
            <ul id="pending-documents"><li>No documents selected yet.</li></ul>
            <p id="document-error" class="error-text" hidden></p>
          </div>
          <button type="submit">Run Intake Pipeline</button>
        </form>
      </div>
      <div class="card">
        <p class="meta">Workspace</p>
        <h2>Recent Bundles</h2>
        <ul>{bundles_html}</ul>
        <p class="meta" style="margin-top: 16px;">Workspace root: {html.escape(str(config.workspace_root))}</p>
      </div>
    </section>
  </div>
</body>
</html>"""


def render_bundle_page(config: LocalAppConfig, bundle_manifest: dict) -> str:
    bundle_id = bundle_manifest["bundle_id"]
    latest_run = next(
        (
            run
            for run in bundle_manifest.get("runs", [])
            if run["run_id"] == bundle_manifest.get("latest_run_id")
        ),
        None,
    )
    workbench_href = f"/bundle/{urllib.parse.quote(bundle_id)}/workbench"
    manifest_href = f"/bundle/{urllib.parse.quote(bundle_id)}/manifest"
    workbench_available = latest_workbench_path(config.workspace_root, bundle_id) is not None
    docs = "\n".join(
        "<li>{name} · {media} · {size} bytes</li>".format(
            name=html.escape(document["stored_filename"]),
            media=html.escape(document["media_type"]),
            size=document["byte_count"],
        )
        for document in bundle_manifest.get("documents", [])
    )
    if not docs:
        docs = "<li>No documents in bundle.</li>"

    run_summary = (
        f"<p><strong>Latest run:</strong> {html.escape(latest_run['run_id'])} · "
        f"{html.escape(latest_run['config']['engine'])} · "
        f"{html.escape(latest_run['filing_status'])}</p>"
        if latest_run
        else "<p><strong>Latest run:</strong> none</p>"
    )

    workbench_panel = (
        f'<iframe title="FA review workbench" src="{workbench_href}"></iframe>'
        if workbench_available
        else (
            '<div class="panel">'
            "<p><strong>Workbench unavailable.</strong></p>"
            "<p>This bundle has not produced a review workbench yet. Run the ingestion pipeline and refresh this page.</p>"
            "</div>"
        )
    )

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{html.escape(bundle_id)} · Expense Local App</title>
  <style>
    body {{ font-family: Georgia, 'Times New Roman', serif; margin: 0; background: #faf7f0; color: #1f1a17; }}
    .shell {{ max-width: 1200px; margin: 0 auto; padding: 24px; }}
    .topbar {{ display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-bottom: 20px; }}
    .panel {{ background: white; border: 1px solid #e4dac9; border-radius: 18px; padding: 18px; box-shadow: 0 10px 20px rgba(0,0,0,0.04); }}
    .grid {{ display: grid; grid-template-columns: 320px 1fr; gap: 18px; }}
    ul {{ padding-left: 18px; }}
    iframe {{ width: 100%; min-height: 88vh; border: 1px solid #d8ccb8; border-radius: 18px; background: white; }}
    a {{ color: #204c63; }}
    @media (max-width: 920px) {{ .grid {{ grid-template-columns: 1fr; }} }}
  </style>
</head>
<body>
  <div class="shell">
    <div class="topbar">
      <div>
        <p style="margin: 0; color: #6b6156;">Managed bundle</p>
        <h1 style="margin: 0;">{html.escape(bundle_id)}</h1>
      </div>
      <p><a href="/">Back to upload</a></p>
    </div>
    <div class="grid">
      <aside class="panel">
        <p><strong>Current stage:</strong> {html.escape(bundle_manifest['current_stage'])}</p>
        {run_summary}
        <p><a href="{workbench_href}">Open workbench only</a></p>
        <p><a href="{manifest_href}">Bundle manifest JSON</a></p>
        <h2>Documents</h2>
        <ul>{docs}</ul>
      </aside>
      <section>
        {workbench_panel}
      </section>
    </div>
  </div>
</body>
</html>"""


def default_bundle_id(input_paths: list[Path]) -> str:
    stem = input_paths[0].stem if input_paths else "bundle"
    return sanitize_identifier(stem)


def sanitize_identifier(value: str) -> str:
    cleaned = []
    previous_separator = False
    for char in value:
        if char.isalnum():
            cleaned.append(char.lower())
            previous_separator = False
        elif not previous_separator:
            cleaned.append("_")
            previous_separator = True
    return "".join(cleaned).strip("_") or "bundle"


class LocalAppHandler(http.server.BaseHTTPRequestHandler):
    server_version = "ExpenseLocalApp/0.1"

    @property
    def config(self) -> LocalAppConfig:
        return self.server.config  # type: ignore[attr-defined]

    def do_GET(self) -> None:
        parsed = urllib.parse.urlparse(self.path)
        try:
            if parsed.path == "/":
                self.respond_html(
                    render_index_page(self.config, list_bundles(self.config.workspace_root))
                )
                return
            if parsed.path.startswith("/bundle/"):
                self.handle_bundle_get(parsed.path)
                return
            self.send_error(404, "Not found")
        except LocalAppError as err:
            self.respond_html(
                render_index_page(
                    self.config,
                    list_bundles(self.config.workspace_root),
                    message=str(err),
                ),
                status=400,
            )

    def do_POST(self) -> None:
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path != "/upload":
            self.send_error(404, "Not found")
            return
        try:
            content_length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(content_length)
            request = parse_multipart_request(
                self.headers.get("Content-Type", ""),
                body,
            )
            bundle_id = handle_upload_submission(
                self.config.repo_root,
                self.config.workspace_root,
                request,
            )
            self.send_response(303)
            self.send_header("Location", f"/bundle/{urllib.parse.quote(bundle_id)}")
            self.end_headers()
        except LocalAppError as err:
            self.respond_html(
                render_index_page(
                    self.config,
                    list_bundles(self.config.workspace_root),
                    message=str(err),
                ),
                status=400,
            )

    def handle_bundle_get(self, path: str) -> None:
        segments = [segment for segment in path.split("/") if segment]
        if len(segments) < 2:
            self.send_error(404, "Not found")
            return
        bundle_id = ensure_safe_bundle_id(urllib.parse.unquote(segments[1]))
        if len(segments) == 2:
            manifest = load_bundle_manifest(self.config.workspace_root, bundle_id)
            self.respond_html(render_bundle_page(self.config, manifest))
            return
        if len(segments) == 3 and segments[2] == "manifest":
            manifest = load_bundle_manifest(self.config.workspace_root, bundle_id)
            self.respond_json(manifest)
            return
        if len(segments) == 3 and segments[2] == "workbench":
            path = latest_workbench_path(self.config.workspace_root, bundle_id)
            if not path:
                raise LocalAppError(f"bundle {bundle_id} does not have a review workbench yet")
            self.respond_file(path, "text/html; charset=utf-8")
            return
        self.send_error(404, "Not found")

    def respond_html(self, body: str, status: int = 200) -> None:
        payload = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def respond_json(self, payload: dict, status: int = 200) -> None:
        encoded = json.dumps(payload, indent=2).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def respond_file(self, path: Path, content_type: str | None = None) -> None:
        payload = path.read_bytes()
        mime = content_type or mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, format: str, *args) -> None:  # noqa: A003
        return


class ThreadingHttpServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def run_server(config: LocalAppConfig) -> None:
    config.workspace_root.mkdir(parents=True, exist_ok=True)
    server = ThreadingHttpServer((config.host, config.port), LocalAppHandler)
    server.config = config  # type: ignore[attr-defined]
    print(f"expense local app listening on http://{config.host}:{config.port}")
    print(f"workspace root: {config.workspace_root}")
    server.serve_forever()


def main() -> int:
    args = parse_args()
    config = LocalAppConfig(
        repo_root=repo_root(),
        workspace_root=Path(args.workspace_root),
        host=args.host,
        port=args.port,
    )
    try:
        run_server(config)
    except KeyboardInterrupt:
        print("\nstopping expense local app")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
