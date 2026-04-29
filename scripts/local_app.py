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
import sys
import tempfile
import time
import urllib.parse
from dataclasses import dataclass
from email.parser import BytesParser
from email.policy import default
from pathlib import Path
from typing import Callable


DEFAULT_HOST = os.environ.get("HOST", "127.0.0.1")
DEFAULT_PORT = int(os.environ.get("PORT", "8765"))
DEFAULT_MODEL = "gemini-3-flash-preview"
DEFAULT_LOCATION = "global"
FAVICON_LINK = (
    '<link rel="icon" href="data:image/svg+xml,'
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'>"
    "<rect rx='20' width='100' height='100' fill='%232f5b53'/>"
    "<text x='50' y='68' text-anchor='middle' fill='white' font-size='52' font-family='Georgia'>$</text>"
    "</svg>\">"
)

EXPORTABLE_ARTIFACTS = {
    "draft.yaml",
    "validation.json",
    "readiness.json",
    "review_packet.json",
    "review_workbench.html",
    "ledger.json",
}

ACTIVE_JOB_STATUSES = {"accepted", "queued", "running", "cancel_requested"}
TERMINAL_JOB_STATUSES = {"ready_for_review", "failed", "canceled", "stalled"}


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
    default_engine: str
    default_fx: str
    default_project: str | None
    default_location: str
    default_model: str
    default_service_account_key: str | None
    default_sdk_python: str | None
    show_advanced_config: bool


class LocalAppError(RuntimeError):
    pass


def jobs_root_path(workspace_root: Path) -> Path:
    return workspace_root / "jobs"


def ensure_safe_job_id(job_id: str) -> str:
    if not job_id:
        raise LocalAppError("job identifier is required")
    if len(job_id) > MAX_IDENTIFIER_LENGTH * 2:
        raise LocalAppError("job identifier is too long")
    if sanitize_identifier(job_id) != job_id:
        raise LocalAppError(f"invalid job identifier: {job_id[:80]}")
    return job_id


def job_record_path(workspace_root: Path, job_id: str) -> Path:
    return jobs_root_path(workspace_root) / f"{ensure_safe_job_id(job_id)}.json"


def job_log_path(workspace_root: Path, job_id: str, stream: str) -> Path:
    return jobs_root_path(workspace_root) / f"{ensure_safe_job_id(job_id)}.{stream}.log"


def build_job_id(bundle_id: str) -> str:
    return sanitize_identifier(f"{bundle_id}_{int(time_now_epoch_ms())}")


def write_job_record(workspace_root: Path, job: dict) -> None:
    jobs_root_path(workspace_root).mkdir(parents=True, exist_ok=True)
    path = job_record_path(workspace_root, str(job["job_id"]))
    path.write_text(json.dumps(job, indent=2))


def load_job_record(workspace_root: Path, job_id: str) -> dict:
    path = job_record_path(workspace_root, job_id)
    if not path.exists():
        raise LocalAppError(f"job {job_id} was not found")
    return json.loads(path.read_text())


def job_is_terminal(job: dict) -> bool:
    return str(job.get("status") or "") in TERMINAL_JOB_STATUSES


def process_is_running(pid: int | None) -> bool:
    if not pid or pid <= 0:
        return False
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def refresh_job_record_state(workspace_root: Path, job: dict) -> dict:
    status = str(job.get("status") or "")
    if status not in ACTIVE_JOB_STATUSES:
        return job
    worker_pid = job.get("worker_pid")
    if isinstance(worker_pid, int) and process_is_running(worker_pid):
        return job
    if status == "accepted":
        return job
    updated = dict(job)
    updated["status"] = "stalled"
    updated["stage"] = "stalled"
    updated["updated_at_epoch_ms"] = int(time_now_epoch_ms())
    updated["error_message"] = updated.get("error_message") or (
        "Processing stopped before the website recorded a final result."
    )
    write_job_record(workspace_root, updated)
    return updated


def ensure_safe_bundle_id(bundle_id: str) -> str:
    if not bundle_id:
        raise LocalAppError("bundle identifier is required")
    if len(bundle_id) > MAX_IDENTIFIER_LENGTH:
        raise LocalAppError(f"bundle identifier is too long (max {MAX_IDENTIFIER_LENGTH} characters)")
    if sanitize_identifier(bundle_id) != bundle_id:
        raise LocalAppError(f"invalid bundle identifier: {bundle_id[:80]}")
    return bundle_id


def parse_args() -> argparse.Namespace:
    env = os.environ
    parser = argparse.ArgumentParser(
        description="Run a tiny local upload app for the expense report ingestion workspace."
    )
    parser.add_argument(
        "--workspace-root",
        default=None,
        help="Directory where bundle workspaces should be stored; defaults to a repo-local workspace",
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
    parser.add_argument(
        "--default-engine",
        choices=["builtin", "vertex-gemini-sdk"],
        default=env.get("EXPENSE_LOCAL_APP_DEFAULT_ENGINE") or (
            "vertex-gemini-sdk"
            if env.get("EXPENSE_LOCAL_APP_SERVICE_ACCOUNT_KEY")
            else "builtin"
        ),
        help="Server-side default ingestion engine for uploads",
    )
    parser.add_argument(
        "--default-fx",
        choices=["demo", "none"],
        default=env.get("EXPENSE_LOCAL_APP_DEFAULT_FX", "demo"),
        help="Server-side default FX mode",
    )
    parser.add_argument(
        "--default-project",
        default=env.get("EXPENSE_LOCAL_APP_PROJECT"),
        help="Optional default Vertex project",
    )
    parser.add_argument(
        "--default-location",
        default=env.get("EXPENSE_LOCAL_APP_LOCATION", DEFAULT_LOCATION),
        help="Default Vertex location for OCR",
    )
    parser.add_argument(
        "--default-model",
        default=env.get("EXPENSE_LOCAL_APP_MODEL", DEFAULT_MODEL),
        help="Default Gemini model for OCR",
    )
    parser.add_argument(
        "--default-service-account-key",
        default=env.get("EXPENSE_LOCAL_APP_SERVICE_ACCOUNT_KEY"),
        help="Default Vertex service account key path",
    )
    parser.add_argument(
        "--default-sdk-python",
        default=env.get("EXPENSE_LOCAL_APP_SDK_PYTHON"),
        help="Default Python interpreter with google-genai installed",
    )
    parser.add_argument(
        "--show-advanced-config",
        action="store_true",
        default=(env.get("EXPENSE_LOCAL_APP_SHOW_ADVANCED_CONFIG") == "1"),
        help="Expose technical ingestion overrides in the upload form",
    )
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def repo_runtime_root(repo_root: Path) -> Path:
    return repo_root / ".local_runtime"


def repo_scoped_tempdir(repo_root: Path, prefix: str) -> tempfile.TemporaryDirectory:
    runtime_root = repo_runtime_root(repo_root)
    runtime_root.mkdir(parents=True, exist_ok=True)
    return tempfile.TemporaryDirectory(prefix=prefix, dir=runtime_root)


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


def latest_ledger_path(workspace_root: Path, bundle_id: str) -> Path | None:
    artifacts_dir = latest_run_artifacts_dir(workspace_root, bundle_id)
    if not artifacts_dir:
        return None
    path = artifacts_dir / "ledger.json"
    return path if path.exists() else None


def bundle_root_path(workspace_root: Path, bundle_id: str) -> Path:
    return workspace_root / "bundles" / ensure_safe_bundle_id(bundle_id)


def bundle_inflight_marker_path(workspace_root: Path, bundle_id: str) -> Path:
    return bundle_root_path(workspace_root, bundle_id) / ".local_app_inflight.json"


def active_bundle_job_if_any(workspace_root: Path, bundle_id: str) -> dict | None:
    marker_path = bundle_inflight_marker_path(workspace_root, bundle_id)
    if not marker_path.exists():
        return None
    try:
        payload = json.loads(marker_path.read_text())
    except json.JSONDecodeError:
        release_bundle_inflight_lock(marker_path)
        return None
    job_id = payload.get("job_id")
    if not isinstance(job_id, str) or not job_id:
        release_bundle_inflight_lock(marker_path)
        return None
    try:
        job = load_job_record(workspace_root, job_id)
    except LocalAppError:
        release_bundle_inflight_lock(marker_path)
        return None
    job = refresh_job_record_state(workspace_root, job)
    if job_is_terminal(job):
        release_bundle_inflight_lock(marker_path)
        return None
    return job


def acquire_bundle_inflight_lock(workspace_root: Path, bundle_id: str, job_id: str) -> Path:
    bundle_root = bundle_root_path(workspace_root, bundle_id)
    try:
        bundle_root.mkdir(parents=True, exist_ok=True)
    except OSError as err:
        raise LocalAppError(
            f"could not create bundle directory: {err.strerror}"
        ) from err
    marker_path = bundle_inflight_marker_path(workspace_root, bundle_id)
    payload = {
        "bundle_id": bundle_id,
        "job_id": ensure_safe_job_id(job_id),
        "started_at_epoch_ms": int(time_now_epoch_ms()),
        "status": "processing",
    }
    try:
        fd = os.open(marker_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL)
    except FileExistsError as err:
        active_job = active_bundle_job_if_any(workspace_root, bundle_id)
        if active_job:
            raise LocalAppError(
                f"bundle {bundle_id} is already processing under job {active_job['job_id']}"
            ) from err
        fd = os.open(marker_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)
    return marker_path


def release_bundle_inflight_lock(marker_path: Path) -> None:
    try:
        marker_path.unlink()
    except FileNotFoundError:
        pass


def bundle_job_page_href(job_id: str) -> str:
    return f"/job/{urllib.parse.quote(ensure_safe_job_id(job_id))}"


def bundle_job_status_href(job_id: str) -> str:
    return f"{bundle_job_page_href(job_id)}/status"


def bundle_document_href(bundle_id: str, document_id: str, stored_filename: str) -> str:
    return "/bundle/{bundle_id}/document/{document_id}/{filename}".format(
        bundle_id=urllib.parse.quote(bundle_id),
        document_id=urllib.parse.quote(document_id),
        filename=urllib.parse.quote(stored_filename),
    )


def bundle_document_path(
    workspace_root: Path, bundle_id: str, document_id: str, stored_filename: str
) -> Path:
    bundle_id = ensure_safe_bundle_id(bundle_id)
    manifest = load_bundle_manifest(workspace_root, bundle_id)
    bundle_root = workspace_root / "bundles" / bundle_id
    document = next(
        (
            item
            for item in manifest.get("documents") or []
            if item.get("document_id") == document_id
            and item.get("stored_filename") == stored_filename
        ),
        None,
    )
    if not document:
        document = next(
            (
                item
                for item in manifest.get("documents") or []
                if item.get("stored_filename") == stored_filename
                and document_id_matches_staged_upload(
                    requested_document_id=document_id,
                    staged_document_id=item.get("document_id", ""),
                    stored_filename=stored_filename,
                )
            ),
            None,
        )
    if not document:
        raise LocalAppError(f"document not found in bundle: {document_id}")

    candidate = bundle_root / document["raw_path"]
    resolved_candidate = candidate.resolve()
    try:
        resolved_candidate.relative_to(bundle_root.resolve())
    except ValueError as err:
        raise LocalAppError("document path escaped bundle root") from err
    if not resolved_candidate.exists():
        raise LocalAppError(f"document file missing for {document_id}")
    return candidate


def document_id_matches_staged_upload(
    *,
    requested_document_id: str,
    staged_document_id: str,
    stored_filename: str,
) -> bool:
    if requested_document_id == staged_document_id:
        return True
    stem_id = sanitize_identifier(Path(stored_filename).stem)
    return (
        requested_document_id == stem_id
        and staged_document_id.startswith(f"{stem_id}_")
    )


def bundle_artifact_path(workspace_root: Path, bundle_id: str, artifact_name: str) -> Path:
    if artifact_name not in EXPORTABLE_ARTIFACTS:
        raise LocalAppError(f"artifact not available for export: {artifact_name}")
    artifacts_dir = latest_run_artifacts_dir(workspace_root, bundle_id)
    if not artifacts_dir:
        raise LocalAppError(f"bundle {bundle_id} does not have a run yet")
    path = artifacts_dir / artifact_name
    if not path.exists():
        raise LocalAppError(f"artifact not found: {artifact_name}")
    return path


def bundle_artifact_relative_path(
    workspace_root: Path, bundle_id: str, artifact_relative_path: str
) -> Path:
    artifacts_dir = latest_run_artifacts_dir(workspace_root, bundle_id)
    if not artifacts_dir:
        raise LocalAppError(f"bundle {bundle_id} does not have a run yet")

    relative = Path(artifact_relative_path)
    if relative.is_absolute():
        raise LocalAppError("artifact path must be relative")
    if ".." in relative.parts:
        raise LocalAppError("artifact path escaped artifacts dir")
    if relative.parts == ("review_workbench.html",):
        return bundle_artifact_path(workspace_root, bundle_id, "review_workbench.html")
    if len(relative.parts) == 1:
        return bundle_artifact_path(workspace_root, bundle_id, relative.name)
    if not relative.parts or relative.parts[0] not in {
        "ocr_pass_comparisons",
        "ocr_grounding",
        "ocr_inspection",
    }:
        raise LocalAppError(f"artifact not available for export: {artifact_relative_path}")

    path = artifacts_dir / relative
    resolved_artifacts_dir = artifacts_dir.resolve()
    resolved_path = path.resolve()
    try:
        resolved_path.relative_to(resolved_artifacts_dir)
    except ValueError as err:
        raise LocalAppError("artifact path escaped artifacts dir") from err
    if not resolved_path.exists():
        raise LocalAppError(f"artifact not found: {artifact_relative_path}")
    return path


def load_review_session_state(workspace_root: Path, bundle_id: str) -> dict:
    ledger_path = latest_ledger_path(workspace_root, bundle_id)
    if not ledger_path:
        raise LocalAppError(f"bundle {bundle_id} does not have a ledger yet")
    ledger = json.loads(ledger_path.read_text())
    current_version_id = ledger["summary"]["current_draft_version_id"]
    current_version = next(
        (
            version
            for version in ledger.get("draft_versions", [])
            if version["version_id"] == current_version_id
        ),
        None,
    )
    if not current_version:
        raise LocalAppError(
            f"bundle {bundle_id} is missing current draft version {current_version_id}"
        )
    review_packet = current_version["review_packet"]
    readiness = current_version["readiness"]
    readiness_issue_counts = {
        "automation_gap_count": readiness["issues"] and sum(
            1 for issue in readiness["issues"] if issue["class"] == "automation_gap"
        )
        or 0,
        "user_input_gap_count": readiness["issues"] and sum(
            1 for issue in readiness["issues"] if issue["class"] == "user_input_required"
        )
        or 0,
        "manual_review_count": readiness["issues"] and sum(
            1 for issue in readiness["issues"] if issue["class"] == "manual_review"
        )
        or 0,
        "other_warning_count": readiness["issues"] and sum(
            1 for issue in readiness["issues"] if issue["class"] == "other_warning"
        )
        or 0,
    }
    return {
        "bundle_id": bundle_id,
        "current_state": ledger["summary"]["current_state"],
        "current_draft_version_id": current_version_id,
        "filing_status": filing_status_from_counts(readiness_issue_counts),
        "issue_count": len(review_packet.get("issues_queue") or []),
        "readiness": readiness_issue_counts,
        "review_packet_summary": review_packet["summary"],
    }


def load_job_state(workspace_root: Path, job_id: str) -> dict:
    job = refresh_job_record_state(workspace_root, load_job_record(workspace_root, job_id))
    bundle_id = ensure_safe_bundle_id(str(job["bundle_id"]))
    payload = {
        "job_id": ensure_safe_job_id(job_id),
        "bundle_id": bundle_id,
        "status": job.get("status"),
        "stage": job.get("stage"),
        "created_at_epoch_ms": job.get("created_at_epoch_ms"),
        "updated_at_epoch_ms": job.get("updated_at_epoch_ms"),
        "error_message": job.get("error_message"),
        "filing_status": job.get("filing_status"),
        "cancel_requested": bool(job.get("cancel_requested")),
        "bundle_href": f"/bundle/{urllib.parse.quote(bundle_id)}",
        "job_href": bundle_job_page_href(job_id),
        "status_href": bundle_job_status_href(job_id),
        "result_href": job.get("result_href"),
        "overview_href": job.get("overview_href"),
    }
    if job.get("latest_run_id"):
        payload["latest_run_id"] = job["latest_run_id"]
    return payload


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
    binary = shutil.which("ingest_bundle_workspace")
    return [binary] if binary else ["cargo", "run", "--bin", "ingest_bundle_workspace", "--"]


def resolve_review_cli_command(repo_root: Path) -> list[str]:
    binary = shutil.which("apply_review_revision_to_artifacts")
    return [binary] if binary else ["cargo", "run", "--bin", "apply_review_revision_to_artifacts", "--"]


def resolve_review_surface_cli_command(repo_root: Path) -> list[str]:
    binary = shutil.which("render_current_review_surface")
    return [binary] if binary else ["cargo", "run", "--bin", "render_current_review_surface", "--"]


def build_stage_command(
    config: LocalAppConfig,
    form_fields: dict[str, str],
    input_paths: list[Path],
) -> list[str]:
    bundle_id = sanitize_identifier(form_fields.get("bundle_id") or default_bundle_id(input_paths))
    command = resolve_cli_command(config.repo_root) + [
        "stage",
        "--workspace-root",
        str(config.workspace_root),
        "--bundle-id",
        bundle_id,
    ]
    if form_fields.get("user_id"):
        command.extend(["--user-id", form_fields["user_id"]])
    command.extend(str(path) for path in input_paths)
    return command


def build_run_command(
    config: LocalAppConfig,
    form_fields: dict[str, str],
    bundle_id: str,
) -> list[str]:
    engine = form_fields.get("engine") or config.default_engine
    command = resolve_cli_command(config.repo_root) + [
        "run",
        "--workspace-root",
        str(config.workspace_root),
        "--bundle-id",
        sanitize_identifier(bundle_id),
        "--fx",
        form_fields.get("fx") or config.default_fx,
        "--engine",
        engine,
    ]
    if form_fields.get("run_id"):
        command.extend(["--run-id", sanitize_identifier(form_fields["run_id"])])
    if engine == "vertex-gemini-sdk":
        command.extend(
            [
                "--location",
                form_fields.get("location") or config.default_location,
                "--model",
                form_fields.get("model") or config.default_model,
            ]
        )
        project = form_fields.get("project") or config.default_project
        service_account_key = (
            form_fields.get("service_account_key") or config.default_service_account_key
        )
        sdk_python = form_fields.get("sdk_python") or config.default_sdk_python
        if project:
            command.extend(["--project", project])
        if service_account_key:
            command.extend(["--service-account-key", service_account_key])
        if sdk_python:
            command.extend(["--sdk-python", sdk_python])
        command.append("--compare-receipt-passes")
    return command


def build_async_job_runner_command(config: LocalAppConfig, job_id: str) -> list[str]:
    return [
        sys.executable,
        str(config.repo_root / "scripts" / "local_app_job_runner.py"),
        "--repo-root",
        str(config.repo_root),
        "--workspace-root",
        str(config.workspace_root),
        "--job-id",
        ensure_safe_job_id(job_id),
    ]


def run_pipeline_command(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, text=True, capture_output=True)


def launch_background_job(
    command: list[str],
    cwd: Path,
    *,
    stdout_path: Path,
    stderr_path: Path,
) -> int:
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    with stdout_path.open("ab") as stdout_handle, stderr_path.open("ab") as stderr_handle:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            stdout=stdout_handle,
            stderr=stderr_handle,
            start_new_session=True,
        )
    return process.pid


def build_review_save_command(
    repo_root: Path,
    artifacts_dir: Path,
    revision_json_path: Path,
    base_version_id: int | None = None,
) -> list[str]:
    command = resolve_review_cli_command(repo_root) + [
        "--artifacts-dir",
        str(artifacts_dir),
        "--revision-json",
        str(revision_json_path),
    ]
    if base_version_id is not None:
        command.extend(["--base-version", str(base_version_id)])
    return command


def build_render_surface_command(
    repo_root: Path,
    artifacts_dir: Path,
    surface: str,
) -> list[str]:
    return resolve_review_surface_cli_command(repo_root) + [
        "--artifacts-dir",
        str(artifacts_dir),
        "--surface",
        surface,
    ]


def sanitize_pipeline_error(raw_output: str) -> str:
    """Extract a user-safe error message from pipeline stderr.

    Strips cargo build output, absolute filesystem paths, and internal
    command details that should not be shown to end users.
    """
    import re

    lines = raw_output.strip().splitlines()
    filtered = []
    for line in lines:
        stripped = line.strip()
        # Skip cargo compilation/build output
        if stripped.startswith(("Compiling ", "Finished ", "Running `", "warning: ", "Downloading ", "Downloaded ")):
            continue
        # Skip empty lines at the start
        if not filtered and not stripped:
            continue
        filtered.append(stripped)

    if not filtered:
        return "The uploaded document could not be processed. Please check that it is a valid PDF."

    message = "\n".join(filtered)
    # Scrub absolute file paths (Unix-style)
    message = re.sub(r"/(?:Users|home|tmp|var|private)[/\w._-]*", "[path]", message)
    # Scrub Windows-style paths just in case
    message = re.sub(r"[A-Z]:\\[\w\\._-]+", "[path]", message)
    # Truncate overly long messages
    if len(message) > 500:
        message = message[:500] + "…"
    return message


def handle_upload_submission(
    config: LocalAppConfig,
    request: UploadRequest,
    command_runner: Callable[[list[str], Path], subprocess.CompletedProcess[str]] = run_pipeline_command,
    job_launcher: Callable[..., int] = launch_background_job,
) -> str:
    if not request.files:
        raise LocalAppError("at least one document upload is required")

    default_bundle = sanitize_identifier(Path(request.files[0].filename).stem)
    bundle_id = sanitize_identifier(request.fields.get("bundle_id") or default_bundle)
    active_job = active_bundle_job_if_any(config.workspace_root, bundle_id)
    if active_job:
        return str(active_job["job_id"])

    job_id = build_job_id(bundle_id)
    marker_path = acquire_bundle_inflight_lock(config.workspace_root, bundle_id, job_id)
    try:
        with repo_scoped_tempdir(config.repo_root, "expense_local_app_") as temp_dir_str:
            temp_dir = Path(temp_dir_str)
            input_paths = save_uploaded_files(temp_dir, request.files)
            stage_command = build_stage_command(config, request.fields, input_paths)
            completed = command_runner(stage_command, config.repo_root)
            if completed.returncode != 0:
                raise LocalAppError(sanitize_pipeline_error(completed.stderr or completed.stdout or ""))
        run_command = build_run_command(config, request.fields, bundle_id)
        job_record = {
            "job_id": job_id,
            "bundle_id": bundle_id,
            "status": "queued",
            "stage": "queued",
            "created_at_epoch_ms": int(time_now_epoch_ms()),
            "updated_at_epoch_ms": int(time_now_epoch_ms()),
            "worker_pid": None,
            "error_message": None,
            "cancel_requested": False,
            "run_command": run_command,
            "stdout_log": str(job_log_path(config.workspace_root, job_id, "stdout")),
            "stderr_log": str(job_log_path(config.workspace_root, job_id, "stderr")),
        }
        write_job_record(config.workspace_root, job_record)
        worker_pid = job_launcher(
            build_async_job_runner_command(config, job_id),
            config.repo_root,
            stdout_path=job_log_path(config.workspace_root, job_id, "stdout"),
            stderr_path=job_log_path(config.workspace_root, job_id, "stderr"),
        )
        job_record["worker_pid"] = worker_pid
        job_record["updated_at_epoch_ms"] = int(time_now_epoch_ms())
        write_job_record(config.workspace_root, job_record)
        return job_id
    except Exception:
        release_bundle_inflight_lock(marker_path)
        raise


def run_job_once(
    repo_root: Path,
    workspace_root: Path,
    job_id: str,
    command_runner: Callable[[list[str], Path], subprocess.CompletedProcess[str]] = run_pipeline_command,
) -> int:
    job = refresh_job_record_state(workspace_root, load_job_record(workspace_root, job_id))
    bundle_id = ensure_safe_bundle_id(str(job["bundle_id"]))
    if job.get("cancel_requested"):
        job["status"] = "canceled"
        job["stage"] = "canceled"
        job["updated_at_epoch_ms"] = int(time_now_epoch_ms())
        write_job_record(workspace_root, job)
        release_bundle_inflight_lock(bundle_inflight_marker_path(workspace_root, bundle_id))
        return 0

    job["status"] = "running"
    job["stage"] = "processing"
    job["updated_at_epoch_ms"] = int(time_now_epoch_ms())
    job["worker_pid"] = os.getpid()
    write_job_record(workspace_root, job)
    try:
        completed = command_runner([str(value) for value in job["run_command"]], repo_root)
        if completed.returncode != 0:
            job["status"] = "failed"
            job["stage"] = "failed"
            job["error_message"] = sanitize_pipeline_error(
                completed.stderr or completed.stdout or ""
            )
            job["updated_at_epoch_ms"] = int(time_now_epoch_ms())
            write_job_record(workspace_root, job)
            return completed.returncode
        manifest = load_bundle_manifest(workspace_root, bundle_id)
        session = load_review_session_state(workspace_root, bundle_id)
        job["status"] = "ready_for_review"
        job["stage"] = "complete"
        job["updated_at_epoch_ms"] = int(time_now_epoch_ms())
        job["latest_run_id"] = manifest.get("latest_run_id")
        job["filing_status"] = session.get("filing_status")
        job["result_href"] = f"/bundle/{urllib.parse.quote(bundle_id)}/workbench"
        job["overview_href"] = f"/bundle/{urllib.parse.quote(bundle_id)}/overview"
        write_job_record(workspace_root, job)
        return 0
    finally:
        release_bundle_inflight_lock(bundle_inflight_marker_path(workspace_root, bundle_id))


def handle_review_save_submission(
    repo_root: Path,
    workspace_root: Path,
    bundle_id: str,
    payload: dict,
    command_runner: Callable[[list[str], Path], subprocess.CompletedProcess[str]] = run_pipeline_command,
) -> dict:
    bundle_id = ensure_safe_bundle_id(bundle_id)
    artifacts_dir = latest_run_artifacts_dir(workspace_root, bundle_id)
    if not artifacts_dir:
        raise LocalAppError(f"bundle {bundle_id} does not have a run yet")

    revision_payload = dict(payload)
    base_version_id = revision_payload.pop("base_version_id", None)
    if base_version_id is not None:
        try:
            base_version_id = int(base_version_id)
        except (TypeError, ValueError) as err:
            raise LocalAppError(f"invalid base version id: {base_version_id}") from err

    with repo_scoped_tempdir(repo_root, "expense_review_save_") as temp_dir_str:
        temp_dir = Path(temp_dir_str)
        revision_json_path = temp_dir / "review_revision.json"
        revision_json_path.write_text(json.dumps(revision_payload, indent=2))
        command = build_review_save_command(
            repo_root,
            artifacts_dir,
            revision_json_path,
            base_version_id=base_version_id,
        )
        completed = command_runner(command, repo_root)
        if completed.returncode != 0:
            raise LocalAppError((completed.stderr or completed.stdout).strip() or "review save failed")
        try:
            result = json.loads(completed.stdout)
        except json.JSONDecodeError as err:
            raise LocalAppError(f"review save produced invalid JSON: {err}") from err

    update_bundle_manifest_after_review_save(workspace_root, bundle_id, result)
    return result


def render_current_review_surface_html(
    repo_root: Path,
    workspace_root: Path,
    bundle_id: str,
    surface: str,
    command_runner: Callable[[list[str], Path], subprocess.CompletedProcess[str]] = run_pipeline_command,
) -> str:
    bundle_id = ensure_safe_bundle_id(bundle_id)
    artifacts_dir = latest_run_artifacts_dir(workspace_root, bundle_id)
    if not artifacts_dir:
        raise LocalAppError(f"bundle {bundle_id} does not have a run yet")
    if not (artifacts_dir / "ledger.json").exists():
        raise LocalAppError(f"bundle {bundle_id} does not have a ledger yet")

    command = build_render_surface_command(repo_root, artifacts_dir, surface)
    completed = command_runner(command, repo_root)
    if completed.returncode != 0:
        raise LocalAppError(
            (completed.stderr or completed.stdout).strip()
            or f"failed to render current review {surface} surface"
        )
    return completed.stdout


def render_current_workbench_html(
    repo_root: Path,
    workspace_root: Path,
    bundle_id: str,
    command_runner: Callable[[list[str], Path], subprocess.CompletedProcess[str]] = run_pipeline_command,
) -> str:
    return render_current_review_surface_html(
        repo_root,
        workspace_root,
        bundle_id,
        "fa",
        command_runner=command_runner,
    )


def update_bundle_manifest_after_review_save(
    workspace_root: Path, bundle_id: str, result: dict
) -> None:
    bundle_id = ensure_safe_bundle_id(bundle_id)
    manifest_path = workspace_root / "bundles" / bundle_id / "bundle_manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["updated_at_epoch_ms"] = int(time_now_epoch_ms())
    manifest["current_stage"] = ledger_state_to_workspace_stage(result["ledger_state"])
    latest_run_id = manifest.get("latest_run_id")
    for run in manifest.get("runs") or []:
        if run.get("run_id") == latest_run_id:
            run["filing_status"] = result["filing_status"]
            run["ledger_state"] = result["ledger_state"]
    manifest_path.write_text(json.dumps(manifest, indent=2))


def ledger_state_to_workspace_stage(ledger_state: str) -> str:
    mapping = {
        "automationblocked": "automation_blocked",
        "automation_blocked": "automation_blocked",
        "userinputrequired": "user_input_required",
        "user_input_required": "user_input_required",
        "manualreviewrequired": "manual_review_required",
        "manual_review_required": "manual_review_required",
        "readytofile": "ready_to_file",
        "ready_to_file": "ready_to_file",
        "submitted": "submitted",
        "accepted": "accepted",
        "returned": "returned",
        "rejected": "rejected",
    }
    return mapping.get(ledger_state, ledger_state)


def filing_status_from_counts(readiness_counts: dict[str, int]) -> str:
    if readiness_counts["automation_gap_count"] > 0:
        return "automation_blocked"
    if readiness_counts["user_input_gap_count"] > 0:
        return "user_input_required"
    if readiness_counts["manual_review_count"] > 0:
        return "manual_review_required"
    return "ready_to_file"


def preview_is_available(session: dict) -> bool:
    readiness = session.get("readiness", {})
    return (
        session.get("filing_status") == "ready_to_file"
        and session.get("issue_count", 0) == 0
        and readiness.get("automation_gap_count", 0) == 0
        and readiness.get("user_input_gap_count", 0) == 0
        and readiness.get("manual_review_count", 0) == 0
    )


def time_now_epoch_ms() -> int:
    return int(time.time() * 1000)


def configured_ingestion_label(config: LocalAppConfig) -> str:
    if config.default_engine == "vertex-gemini-sdk":
        return "AI Document Processing"
    return "Built-in text extraction"


def friendly_stage_label(stage: str) -> str:
    labels = {
        "automation_blocked": "Action Required",
        "user_input_required": "Needs Your Input",
        "manual_review_required": "Ready for Review",
        "ready_to_file": "Ready to File",
        "submitted": "Submitted",
        "accepted": "Accepted",
        "returned": "Returned",
        "rejected": "Rejected",
    }
    return labels.get(stage, stage.replace("_", " ").title())


def format_file_size(byte_count: int) -> str:
    if byte_count < 1024:
        return f"{byte_count} bytes"
    if byte_count < 1024 * 1024:
        return f"{byte_count / 1024:.1f} KB"
    return f"{byte_count / (1024 * 1024):.1f} MB"


def pluralize(count: int, singular: str, plural: str | None = None) -> str:
    word = singular if count == 1 else (plural or f"{singular}s")
    return f"{count} {word}"


def format_epoch_ms(epoch_ms: int) -> str:
    """Format an epoch-millisecond timestamp as a human-readable date/time."""
    ts = time.localtime(epoch_ms / 1000)
    return time.strftime("%b %d, %Y at %I:%M %p", ts)


def render_index_page(config: LocalAppConfig, bundles: list[BundleListEntry], message: str | None = None) -> str:
    items = []
    for bundle in bundles[:30]:
        items.append(
            "<li><a href=\"/bundle/{bundle_id}\">{bundle_id}</a> · {stage} · {documents}</li>".format(
                bundle_id=html.escape(bundle.bundle_id),
                stage=html.escape(friendly_stage_label(bundle.current_stage)),
                documents=pluralize(bundle.document_count, "document"),
            )
        )
    bundles_html = "\n".join(items) or "<li>No bundles yet.</li>"
    if config.show_advanced_config:
        secondary_panel = f"""
      <div class="card">
        <p class="meta">Workspace</p>
        <h2>Recent Bundles</h2>
        <ul>{bundles_html}</ul>
      </div>"""
    else:
        secondary_panel = """
      <div class="card">
        <p class="meta">What Happens Next</p>
        <h2>Review before filing</h2>
        <ol style="margin: 0; padding-left: 18px; line-height: 1.65;">
          <li>Upload your receipts and travel documents.</li>
          <li>Review the extracted fields in the workbench.</li>
          <li>Fill any missing information and save your changes.</li>
          <li>Open the final preview and print or save it as a PDF.</li>
        </ol>
      </div>"""

    notice = (
        f"<p class=\"notice\">{html.escape(message)}</p>\n" if message else ""
    )
    advanced_config = ""
    if config.show_advanced_config:
        advanced_config = f"""
          <details class="advanced-config">
            <summary>Technical overrides</summary>
            <div class="grid">
              <label>Run Label
                <input name="run_id" placeholder="optional_run_id (leave blank to auto-generate)">
              </label>
              <label>FX Mode
                <select name="fx">
                  <option value="demo" {"selected" if config.default_fx == "demo" else ""}>demo</option>
                  <option value="none" {"selected" if config.default_fx == "none" else ""}>none</option>
                </select>
              </label>
              <label>Engine
                <select name="engine">
                  <option value="builtin" {"selected" if config.default_engine == "builtin" else ""}>builtin</option>
                  <option value="vertex-gemini-sdk" {"selected" if config.default_engine == "vertex-gemini-sdk" else ""}>vertex-gemini-sdk</option>
                </select>
              </label>
              <label>Project
                <input name="project" value="{html.escape(config.default_project or '')}" placeholder="optional Vertex project">
              </label>
              <label>Location
                <input name="location" value="{html.escape(config.default_location)}">
              </label>
              <label>Model
                <input name="model" value="{html.escape(config.default_model)}">
              </label>
              <label>Service Account Key
                <input name="service_account_key" value="{html.escape(config.default_service_account_key or '')}" placeholder="/abs/path/to/service-account.json">
              </label>
              <label>SDK Python
                <input name="sdk_python" value="{html.escape(config.default_sdk_python or '')}" placeholder="./.venv/bin/python">
              </label>
            </div>
          </details>
        """

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Stanford Expense Reports</title>
  {FAVICON_LINK}
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
    .ingestion-chip {{ display: inline-flex; align-items: center; gap: 8px; padding: 8px 12px; border-radius: 999px; border: 1px solid #d4c4ac; background: #f7f1e5; color: #5d5349; font-size: 0.94rem; }}
    .advanced-config {{ padding: 12px 14px; border: 1px solid #dbcdb7; border-radius: 14px; background: rgba(247, 241, 229, 0.72); }}
    .advanced-config summary {{ cursor: pointer; font-weight: 700; }}
    .advanced-config .grid {{ margin-top: 12px; }}
    @media (max-width: 920px) {{ .hero, .grid {{ grid-template-columns: 1fr; }} }}
  </style>
  <script>
    const pendingFiles = [];

    function pendingLabel(file) {{
      const sz = file.size;
      const label = sz < 1024 ? sz + ' bytes' : sz < 1048576 ? (sz/1024).toFixed(1) + ' KB' : (sz/1048576).toFixed(1) + ' MB';
      return `${{file.name}} (${{label}})`;
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

    function setProcessingState(active) {{
      const submit = document.querySelector('#upload-form button[type="submit"]');
      const note = document.getElementById('processing-note');
      if (submit) {{
        submit.disabled = active;
        submit.textContent = active ? 'Processing…' : 'Upload & Process';
      }}
      if (note) {{
        note.hidden = !active;
      }}
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
          setProcessingState(false);
          return;
        }}
        clearDocumentError();
        setProcessingState(true);
      }});
      renderPendingUploads();
    }});
  </script>
</head>
<body>
  <div class="shell">
    <section class="hero">
      <div class="card">
        <p class="meta">Expense Reports</p>
        <h1>Upload & Process</h1>
        <p>Upload travel receipts and documents. The system will extract key fields and open an editable workbench for review before filing.</p>
        <p class="ingestion-chip">Accepts PDF, PNG, JPG, HEIC, and text documents</p>
        {notice}
        <form id="upload-form" method="post" action="/upload" enctype="multipart/form-data">
          <div class="grid">
            <label>Report Name
              <input name="bundle_id" placeholder="e.g. ICLR 2025 trip">
            </label>
            <label>Your Name or ID
              <input name="user_id" placeholder="fa_or_requester_id">
            </label>
          </div>
          <label>Documents
            <input id="documents-input" type="file" name="documents" multiple accept=".pdf,.png,.jpg,.jpeg,.heic,.heif,.txt,.md,.markdown">
          </label>
          <div class="pending-uploads">
            <p class="meta">Pending uploads. You can reopen the file picker and selections will accumulate until you submit.</p>
            <ul id="pending-documents"><li>No documents selected yet.</li></ul>
            <p id="document-error" class="error-text" hidden></p>
            <p id="processing-note" class="meta" hidden>Processing your documents. This can take a minute for PDFs, images, and larger uploads. Keep this tab open.</p>
          </div>
          {advanced_config}
          <button type="submit">Upload &amp; Process</button>
        </form>
      </div>
      {secondary_panel}
    </section>
  </div>
</body>
</html>"""


def render_job_page(config: LocalAppConfig, job: dict) -> str:
    bundle_id = html.escape(str(job["bundle_id"]))
    job_id = html.escape(str(job["job_id"]))
    status = html.escape(str(job.get("status") or "accepted").replace("_", " "))
    stage = html.escape(str(job.get("stage") or "accepted").replace("_", " "))
    error_message = job.get("error_message")
    result_href = job.get("result_href")
    overview_href = job.get("overview_href")
    state_copy = {
        "accepted": "We received your upload and are getting it ready.",
        "queued": "Your receipt is queued for processing. This page is safe to refresh.",
        "running": "We are processing your receipt now. You can refresh this page without starting over.",
        "ready_for_review": "Your receipt is ready for review.",
        "failed": "This run failed before a review packet was ready.",
        "stalled": "Processing stopped before the website recorded a final result.",
        "canceled": "This run was canceled.",
    }.get(str(job.get("status") or ""), "We are checking the status of your receipt.")
    error_block = (
        f"<p class=\"notice error\">{html.escape(str(error_message))}</p>"
        if error_message
        else ""
    )
    primary_action = ""
    if result_href:
        primary_action = (
            f"<a class=\"button\" href=\"{html.escape(str(result_href))}\">Open review workbench</a>"
        )
    elif overview_href:
        primary_action = (
            f"<a class=\"button secondary\" href=\"{html.escape(str(overview_href))}\">Open bundle overview</a>"
        )
    refresh_js = ""
    if str(job.get("status")) in ACTIVE_JOB_STATUSES:
        refresh_js = f"""
<script>
async function pollJobStatus(){{
  try {{
    const response = await fetch('{html.escape(str(job['status_href']))}', {{headers: {{'Accept': 'application/json'}}}});
    if(!response.ok) {{
      return;
    }}
    const payload = await response.json();
    const status = payload.status || 'accepted';
    document.getElementById('job-status').textContent = status.replaceAll('_',' ');
    document.getElementById('job-stage').textContent = (payload.stage || status).replaceAll('_',' ');
    if(payload.error_message) {{
      const error = document.getElementById('job-error');
      if(error) {{
        error.hidden = false;
        error.textContent = payload.error_message;
      }}
    }}
    if(status === 'ready_for_review' && payload.result_href) {{
      window.location.href = payload.result_href;
      return;
    }}
    if(status === 'failed' || status === 'stalled' || status === 'canceled') {{
      window.location.reload();
      return;
    }}
  }} catch (err) {{
    // keep polling quietly; transient network errors should not break the page
  }}
}}
setInterval(pollJobStatus, 3000);
</script>"""
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Processing receipt — Stanford Expense Reports</title>
  {FAVICON_LINK}
  <style>
    :root{{font-family:Georgia,serif;color:#211b10;background:#f7f2e8;}}
    body{{margin:0;background:linear-gradient(180deg,#f7f2e8 0%,#efe4d1 100%);color:#211b10;}}
    .shell{{max-width:900px;margin:0 auto;padding:48px 20px 72px;}}
    .card{{background:#fff;border:1px solid #e3dac9;border-radius:24px;box-shadow:0 14px 32px rgba(41,27,16,.08);padding:28px;}}
    h1{{font-size:48px;line-height:1.05;margin:0 0 12px;}}
    .eyebrow{{letter-spacing:.16em;text-transform:uppercase;color:#a35d34;font-weight:700;font-size:13px;margin:0 0 14px;}}
    .meta{{color:#665f57;font-size:18px;line-height:1.6;}}
    .grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:16px;margin:24px 0;}}
    .pill{{display:inline-block;padding:8px 14px;border-radius:999px;background:#f3ece0;border:1px solid #e3dac9;font-size:14px;font-weight:700;text-transform:uppercase;letter-spacing:.08em;color:#665f57;}}
    .button{{display:inline-block;padding:12px 18px;border-radius:999px;background:#2f5b53;color:#fff;text-decoration:none;font-weight:700;margin-right:12px;}}
    .button.secondary{{background:#f3ece0;color:#211b10;border:1px solid #e3dac9;}}
    .notice.error{{margin:18px 0 0;padding:14px 16px;border-radius:16px;background:#f9ece8;border:1px solid #e8c1b6;color:#8a3d23;}}
  </style>
</head>
<body>
  <div class="shell">
    <div class="card">
      <p class="eyebrow">Receipt processing</p>
      <h1>Processing your receipt</h1>
      <p class="meta">{html.escape(state_copy)}</p>
      <div class="grid">
        <div><span class="pill">Bundle</span><p class="meta">{bundle_id}</p></div>
        <div><span class="pill">Job</span><p class="meta">{job_id}</p></div>
        <div><span class="pill">Status</span><p class="meta" id="job-status">{status}</p></div>
        <div><span class="pill">Stage</span><p class="meta" id="job-stage">{stage}</p></div>
      </div>
      {primary_action}
      <a class="button secondary" href="{html.escape(str(job['bundle_href']))}">Open bundle</a>
      {error_block}
      <p class="notice error" id="job-error" hidden></p>
    </div>
  </div>
  {refresh_js}
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
    preview_href = f"/bundle/{urllib.parse.quote(bundle_id)}/preview"
    developer_href = f"/bundle/{urllib.parse.quote(bundle_id)}/developer"
    overview_href = f"/bundle/{urllib.parse.quote(bundle_id)}/overview"
    manifest_href = f"/bundle/{urllib.parse.quote(bundle_id)}/manifest"
    session_href = f"/bundle/{urllib.parse.quote(bundle_id)}/review-session"
    draft_href = f"/bundle/{urllib.parse.quote(bundle_id)}/artifact/draft.yaml"
    packet_href = f"/bundle/{urllib.parse.quote(bundle_id)}/artifact/review_packet.json"
    ledger_href = f"/bundle/{urllib.parse.quote(bundle_id)}/artifact/ledger.json"
    workbench_available = latest_ledger_path(config.workspace_root, bundle_id) is not None
    filing_status = "unknown"
    preview_available = False
    if workbench_available:
        try:
            session = load_review_session_state(config.workspace_root, bundle_id)
            filing_status = session.get("filing_status", "unknown")
            preview_available = preview_is_available(session)
        except LocalAppError:
            pass
    elif latest_run and latest_run.get("filing_status"):
        filing_status = ledger_state_to_workspace_stage(latest_run["filing_status"])
    docs = "\n".join(
        "<li><a href=\"{href}\" target=\"_blank\" rel=\"noreferrer\">{name}</a> · {size}</li>".format(
            href=html.escape(
                bundle_document_href(
                    bundle_id,
                    document["document_id"],
                    document["stored_filename"],
                )
            ),
            name=html.escape(document["stored_filename"]),
            size=format_file_size(document["byte_count"]),
        )
        for document in bundle_manifest.get("documents", [])
    )
    if not docs:
        docs = "<li>No documents in bundle.</li>"

    updated_label = format_epoch_ms(bundle_manifest.get("updated_at_epoch_ms", 0))
    run_summary = (
        f"<p><strong>Last processed:</strong> {html.escape(updated_label)}</p>"
        if latest_run
        else "<p><strong>Last processed:</strong> not yet</p>"
    )

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{html.escape(bundle_id)} — Stanford Expense Reports</title>
  {FAVICON_LINK}
  <style>
    body {{ font-family: Georgia, 'Times New Roman', serif; margin: 0; background: #faf7f0; color: #1f1a17; }}
    .shell {{ max-width: 1200px; margin: 0 auto; padding: 24px; }}
    .topbar {{ display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-bottom: 20px; }}
    .panel {{ background: white; border: 1px solid #e4dac9; border-radius: 18px; padding: 18px; box-shadow: 0 10px 20px rgba(0,0,0,0.04); }}
    .grid {{ display: grid; grid-template-columns: 360px 1fr; gap: 18px; }}
    ul {{ padding-left: 18px; }}
    a {{ color: #204c63; }}
    .primary-link {{ display: inline-flex; margin-top: 10px; padding: 10px 14px; border-radius: 999px; background: #2f5b53; color: white; text-decoration: none; font-weight: 700; }}
    .meta-grid {{ display: grid; gap: 12px; }}
    @media (max-width: 920px) {{ .grid {{ grid-template-columns: 1fr; }} }}
  </style>
</head>
<body>
  <div class="shell">
    <div class="topbar">
      <div>
        <p style="margin: 0; color: #6b6156;">Expense Report</p>
        <h1 style="margin: 0;">{html.escape(bundle_id)}</h1>
      </div>
      <p><a href="/">Back to upload</a></p>
    </div>
    <div class="grid">
      <aside class="panel">
        <p><strong>Status:</strong> {html.escape(friendly_stage_label(filing_status if filing_status != "unknown" else bundle_manifest['current_stage']))}</p>
        {run_summary}
        <p><a class="primary-link" href="{workbench_href}">Open Workbench</a></p>
        <p>{"<a href=\"" + preview_href + "\">Open Final Preview</a>" if preview_available else "<span style=\"opacity: 0.5;\">Open Final Preview (not yet available)</span>"}</p>
        <p style="margin-top: 8px;"><button onclick="location.reload()" style="cursor: pointer; font: inherit; background: transparent; color: #204c63; border: 1px solid #c9bca8; border-radius: 999px; padding: 6px 14px;">Check for Updates</button></p>
        <details style="margin-top: 14px; padding: 10px 12px; border: 1px solid #dbcdb7; border-radius: 12px; background: rgba(247,241,229,0.72);">
          <summary style="cursor: pointer; font-weight: 600; color: #6b6156;">Advanced Tools</summary>
          <div style="margin-top: 8px;">
            <p><a href="{developer_href}">Open developer workbench</a></p>
            <p><a href="{manifest_href}">Bundle manifest JSON</a></p>
            <p><a href="{session_href}">Review session JSON</a></p>
            <p><a href="{draft_href}">Export current draft YAML</a></p>
            <p><a href="{packet_href}">Export review packet JSON</a></p>
            <p><a href="{ledger_href}">Export ledger JSON</a></p>
          </div>
        </details>
        <h2>Documents</h2>
        <ul>{docs}</ul>
      </aside>
      <section class="panel meta-grid">
        <div>
          <p style="margin: 0 0 6px;"><strong>How to complete your report</strong></p>
          <ol style="margin: 0; padding-left: 18px;">
            <li>Open the workbench and fill in any missing fields.</li>
            <li>Click Save Changes when you are done.</li>
            <li>Open the final preview to check everything looks correct.</li>
            <li>Print or save the preview as a PDF for your records.</li>
          </ol>
        </div>
        <div>
          <p><strong>Workbench:</strong> {"Ready" if workbench_available else "Not yet generated"}</p>
          <p><strong>Documents:</strong> {pluralize(len(bundle_manifest.get("documents", [])), "file")}</p>
        </div>
      </section>
    </div>
  </div>
</body>
</html>"""


def render_preview_gate_page(config: LocalAppConfig, bundle_id: str, session: dict) -> str:
    readiness = session.get("readiness", {})
    workbench_href = f"/bundle/{urllib.parse.quote(bundle_id)}/workbench"
    overview_href = f"/bundle/{urllib.parse.quote(bundle_id)}/overview"
    status_label = friendly_stage_label(session.get("filing_status", "unknown"))
    issue_count = session.get("issue_count", 0)
    automation = readiness.get("automation_gap_count", 0)
    user_input = readiness.get("user_input_gap_count", 0)
    manual_review = readiness.get("manual_review_count", 0)

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Preview Not Available — {html.escape(bundle_id)}</title>
  {FAVICON_LINK}
  <style>
    body {{ font-family: Georgia, 'Times New Roman', serif; margin: 0; background: #faf7f0; color: #1f1a17; }}
    .shell {{ max-width: 680px; margin: 0 auto; padding: 48px 24px; }}
    .card {{ background: white; border: 1px solid #e4dac9; border-radius: 18px; padding: 28px; box-shadow: 0 10px 20px rgba(0,0,0,0.04); }}
    h1 {{ margin: 0 0 8px; }}
    p {{ line-height: 1.6; }}
    .status-chip {{ display: inline-block; padding: 6px 14px; border-radius: 999px; background: #fef3cd; color: #856404; font-weight: 600; font-size: 0.95rem; }}
    .readiness-grid {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; margin: 18px 0; }}
    .readiness-item {{ padding: 14px; border-radius: 12px; background: #f8f5ee; text-align: center; }}
    .readiness-item .count {{ font-size: 1.8rem; font-weight: 700; color: #2f5b53; }}
    .readiness-item .label {{ font-size: 0.85rem; color: #6b6156; margin-top: 4px; }}
    a {{ color: #204c63; }}
    .primary-link {{ display: inline-flex; margin-top: 14px; padding: 10px 16px; border-radius: 999px; background: #2f5b53; color: white; text-decoration: none; font-weight: 700; }}
    .meta {{ color: #6b6156; font-size: 0.95rem; }}
  </style>
</head>
<body>
  <div class="shell">
    <div class="card">
      <p class="meta">Report: {html.escape(bundle_id)}</p>
      <h1>Preview Not Yet Available</h1>
      <p>The final preview is available once all required fields are filled and all items are reviewed. The current status is <span class="status-chip">{html.escape(status_label)}</span> with {pluralize(issue_count, "item")} remaining.</p>
      <div class="readiness-grid">
        <div class="readiness-item">
          <div class="count">{automation}</div>
          <div class="label">Could Not Extract</div>
        </div>
        <div class="readiness-item">
          <div class="count">{user_input}</div>
          <div class="label">Needs Your Input</div>
        </div>
        <div class="readiness-item">
          <div class="count">{manual_review}</div>
          <div class="label">Review Required</div>
        </div>
      </div>
      <p>Open the workbench to fill in missing fields and review flagged items, then return here to preview the final filing.</p>
      <a class="primary-link" href="{workbench_href}">Open Workbench</a>
      <p style="margin-top: 16px;"><a href="{overview_href}">Back to bundle overview</a></p>
    </div>
  </div>
</body>
</html>"""


def build_debug_metrics(config: LocalAppConfig) -> dict:
    """Build a metrics summary for the /debug/metrics endpoint."""
    bundles = list_bundles(config.workspace_root)
    stage_counts: dict[str, int] = {}
    total_documents = 0
    for b in bundles:
        stage_counts[b.current_stage] = stage_counts.get(b.current_stage, 0) + 1
        total_documents += b.document_count

    workspace_size = 0
    bundles_dir = config.workspace_root / "bundles"
    if bundles_dir.exists():
        for f in bundles_dir.rglob("*"):
            if f.is_file():
                try:
                    workspace_size += f.stat().st_size
                except OSError:
                    pass

    return {
        "bundle_count": len(bundles),
        "total_documents": total_documents,
        "stages": stage_counts,
        "workspace_size_bytes": workspace_size,
        "workspace_size_human": format_file_size(workspace_size),
        "engine": config.default_engine,
        "server_time": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
    }


def build_debug_bundles(config: LocalAppConfig) -> list[dict]:
    """Build detailed bundle state for the /debug/bundles endpoint."""
    bundles = list_bundles(config.workspace_root)
    result = []
    for b in bundles:
        try:
            manifest = load_bundle_manifest(config.workspace_root, b.bundle_id)
        except LocalAppError:
            manifest = {}

        runs = manifest.get("runs", [])
        entry: dict = {
            "bundle_id": b.bundle_id,
            "current_stage": b.current_stage,
            "document_count": b.document_count,
            "run_count": len(runs),
            "updated_at": format_epoch_ms(b.updated_at_epoch_ms),
            "updated_at_epoch_ms": b.updated_at_epoch_ms,
        }
        if runs:
            latest = runs[-1]
            entry["latest_run"] = {
                "run_id": latest.get("run_id"),
                "engine": latest.get("config", {}).get("engine"),
                "filing_status": latest.get("filing_status"),
            }

        try:
            session = load_review_session_state(config.workspace_root, b.bundle_id)
            entry["review_session"] = {
                "filing_status": session.get("filing_status"),
                "issue_count": session.get("issue_count"),
                "readiness": session.get("readiness"),
                "version_count": session.get("version_count"),
            }
        except LocalAppError:
            entry["review_session"] = None

        result.append(entry)
    return result


def render_error_page(status_code: int, message: str) -> str:
    """Render a branded error page that replaces Python's default."""
    friendly = {
        400: "Bad Request",
        404: "Page Not Found",
        405: "Method Not Allowed",
        500: "Internal Server Error",
    }
    title = friendly.get(status_code, f"Error {status_code}")
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{status_code} — {html.escape(title)}</title>
  {FAVICON_LINK}
  <style>
    body {{ font-family: Georgia, 'Times New Roman', serif; margin: 0; background: #faf7f0; color: #1f1a17; }}
    .shell {{ max-width: 560px; margin: 0 auto; padding: 80px 24px; text-align: center; }}
    .code {{ font-size: 4rem; font-weight: 700; color: #2f5b53; margin: 0; }}
    h1 {{ margin: 8px 0 16px; }}
    p {{ line-height: 1.6; color: #5d5349; }}
    a {{ color: #204c63; }}
  </style>
</head>
<body>
  <div class="shell">
    <p class="code">{status_code}</p>
    <h1>{html.escape(title)}</h1>
    <p>{html.escape(message)}</p>
    <p><a href="/">Back to upload page</a></p>
  </div>
</body>
</html>"""


def default_bundle_id(input_paths: list[Path]) -> str:
    stem = input_paths[0].stem if input_paths else "bundle"
    return sanitize_identifier(stem)


MAX_IDENTIFIER_LENGTH = 200


def sanitize_identifier(value: str) -> str:
    cleaned = []
    previous_separator = False
    for char in value:
        if char.isascii() and char.isalnum():
            cleaned.append(char.lower())
            previous_separator = False
        elif not previous_separator:
            cleaned.append("_")
            previous_separator = True
    result = "".join(cleaned).strip("_") or "bundle"
    return result[:MAX_IDENTIFIER_LENGTH]


class LocalAppHandler(http.server.BaseHTTPRequestHandler):
    server_version = "ExpenseLocalApp"

    def version_string(self) -> str:
        return self.server_version

    @property
    def config(self) -> LocalAppConfig:
        return self.server.config  # type: ignore[attr-defined]

    def do_HEAD(self) -> None:
        """Handle HEAD requests by running GET logic with body suppressed."""
        self._suppress_body = True
        try:
            self.do_GET()
        finally:
            self._suppress_body = False

    def send_error(self, code: int, message: str | None = None, explain: str | None = None) -> None:
        """Send a styled error page instead of Python's default."""
        short_msg = message or "Error"
        body = render_error_page(code, short_msg)
        payload = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(payload)

    def do_GET(self) -> None:
        parsed = urllib.parse.urlparse(self.path)
        try:
            if parsed.path == "/":
                self.respond_html(
                    render_index_page(self.config, list_bundles(self.config.workspace_root))
                )
                return
            if parsed.path.startswith("/debug/"):
                self.handle_debug_get(parsed.path)
                return
            if parsed.path.startswith("/job/"):
                self.handle_job_get(parsed.path)
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

    def _read_request_body(self) -> bytes:
        """Read the full request body.

        Cloud Run always forwards HTTP/1.1 with Content-Length to containers,
        but we handle chunked transfer as a safety net.
        """
        content_length = self.headers.get("Content-Length")
        if content_length is not None:
            return self.rfile.read(int(content_length))
        transfer_encoding = self.headers.get("Transfer-Encoding", "")
        if "chunked" in transfer_encoding.lower():
            chunks = []
            while True:
                line = self.rfile.readline().strip()
                chunk_size = int(line, 16)
                if chunk_size == 0:
                    self.rfile.readline()
                    break
                chunks.append(self.rfile.read(chunk_size))
                self.rfile.readline()
            return b"".join(chunks)
        return b""

    def do_POST(self) -> None:
        parsed = urllib.parse.urlparse(self.path)
        is_review_save = parsed.path.startswith("/bundle/") and parsed.path.endswith("/review-session/save")
        try:
            if parsed.path == "/upload":
                body = self._read_request_body()
                request = parse_multipart_request(
                    self.headers.get("Content-Type", ""),
                    body,
                )
                job_id = handle_upload_submission(
                    self.config,
                    request,
                )
                self.send_response(303)
                self.send_header("Location", bundle_job_page_href(job_id))
                self.end_headers()
                return
            if parsed.path.startswith("/bundle/") and parsed.path.endswith("/review-session/save"):
                segments = [segment for segment in parsed.path.split("/") if segment]
                if len(segments) != 4:
                    self.send_error(404, "Not found")
                    return
                bundle_id = ensure_safe_bundle_id(urllib.parse.unquote(segments[1]))
                payload = json.loads(self._read_request_body() or b"{}")
                result = handle_review_save_submission(
                    self.config.repo_root,
                    self.config.workspace_root,
                    bundle_id,
                    payload,
                )
                self.respond_json(result)
                return
            self.send_error(405, "This endpoint does not accept POST requests")
        except LocalAppError as err:
            if is_review_save:
                self.respond_json({"error": str(err)}, status=400)
            else:
                self.respond_html(
                    render_index_page(
                        self.config,
                        list_bundles(self.config.workspace_root),
                        message=str(err),
                    ),
                    status=400,
                )
        except json.JSONDecodeError as err:
            self.respond_json({"error": f"invalid json: {err}"}, status=400)

    def handle_bundle_get(self, path: str) -> None:
        segments = [segment for segment in path.split("/") if segment]
        if len(segments) < 2:
            self.send_error(404, "Not found")
            return
        bundle_id = ensure_safe_bundle_id(urllib.parse.unquote(segments[1]))
        if len(segments) == 2:
            active_job = active_bundle_job_if_any(self.config.workspace_root, bundle_id)
            if active_job:
                self.send_response(303)
                self.send_header(
                    "Location",
                    bundle_job_page_href(str(active_job["job_id"])),
                )
                self.end_headers()
                return
            if latest_ledger_path(self.config.workspace_root, bundle_id):
                self.send_response(303)
                self.send_header(
                    "Location", f"/bundle/{urllib.parse.quote(bundle_id)}/workbench"
                )
                self.end_headers()
            else:
                manifest = load_bundle_manifest(self.config.workspace_root, bundle_id)
                self.respond_html(render_bundle_page(self.config, manifest))
            return
        if len(segments) == 3 and segments[2] == "overview":
            manifest = load_bundle_manifest(self.config.workspace_root, bundle_id)
            self.respond_html(render_bundle_page(self.config, manifest))
            return
        if len(segments) == 3 and segments[2] == "manifest":
            manifest = load_bundle_manifest(self.config.workspace_root, bundle_id)
            self.respond_json(manifest)
            return
        if len(segments) == 3 and segments[2] == "review-session":
            self.respond_json(load_review_session_state(self.config.workspace_root, bundle_id))
            return
        if len(segments) == 3 and segments[2] == "workbench":
            body = render_current_review_surface_html(
                self.config.repo_root,
                self.config.workspace_root,
                bundle_id,
                "fa",
            )
            self.respond_html(body)
            return
        if len(segments) == 3 and segments[2] == "preview":
            session = load_review_session_state(self.config.workspace_root, bundle_id)
            if not preview_is_available(session):
                self.respond_html(
                    render_preview_gate_page(self.config, bundle_id, session)
                )
                return
            body = render_current_review_surface_html(
                self.config.repo_root,
                self.config.workspace_root,
                bundle_id,
                "preview",
            )
            self.respond_html(body)
            return
        if len(segments) == 3 and segments[2] == "developer":
            body = render_current_review_surface_html(
                self.config.repo_root,
                self.config.workspace_root,
                bundle_id,
                "developer",
            )
            self.respond_html(body)
            return
        if len(segments) >= 4 and segments[2] == "artifact":
            artifact_name = "/".join(urllib.parse.unquote(segment) for segment in segments[3:])
            if artifact_name == "review_workbench.html":
                body = render_current_review_surface_html(
                    self.config.repo_root,
                    self.config.workspace_root,
                    bundle_id,
                    "developer",
                )
                self.respond_html(body)
            else:
                path = bundle_artifact_relative_path(
                    self.config.workspace_root,
                    bundle_id,
                    artifact_name,
                )
                self.respond_file(path)
            return
        if len(segments) == 5 and segments[2] == "document":
            document_id = urllib.parse.unquote(segments[3])
            stored_filename = urllib.parse.unquote(segments[4])
            path = bundle_document_path(
                self.config.workspace_root,
                bundle_id,
                document_id,
                stored_filename,
            )
            self.respond_file(path)
            return
        self.send_error(404, "Not found")

    def handle_job_get(self, path: str) -> None:
        segments = [segment for segment in path.split("/") if segment]
        if len(segments) < 2:
            self.send_error(404, "Not found")
            return
        job_id = ensure_safe_job_id(urllib.parse.unquote(segments[1]))
        if len(segments) == 2:
            self.respond_html(render_job_page(self.config, load_job_state(self.config.workspace_root, job_id)))
            return
        if len(segments) == 3 and segments[2] == "status":
            self.respond_json(load_job_state(self.config.workspace_root, job_id))
            return
        self.send_error(404, "Not found")

    def handle_debug_get(self, path: str) -> None:
        segments = [s for s in path.split("/") if s]
        if len(segments) == 2 and segments[1] == "metrics":
            self.respond_json(build_debug_metrics(self.config))
            return
        if len(segments) == 2 and segments[1] == "bundles":
            self.respond_json(build_debug_bundles(self.config))
            return
        self.send_error(404, "Not found")

    def respond_html(self, body: str, status: int = 200) -> None:
        payload = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_cache_busting_headers()
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        if not getattr(self, "_suppress_body", False):
            self.wfile.write(payload)

    def respond_json(self, payload, status: int = 200) -> None:
        encoded = json.dumps(payload, indent=2).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_cache_busting_headers()
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        if not getattr(self, "_suppress_body", False):
            self.wfile.write(encoded)

    def respond_file(self, path: Path, content_type: str | None = None) -> None:
        payload = path.read_bytes()
        mime = content_type or mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_cache_busting_headers()
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        if not getattr(self, "_suppress_body", False):
            self.wfile.write(payload)

    def send_cache_busting_headers(self) -> None:
        self.send_header("Cache-Control", "no-store, no-cache, must-revalidate, max-age=0")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")

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
    root = repo_root()
    config = LocalAppConfig(
        repo_root=root,
        workspace_root=Path(args.workspace_root)
        if args.workspace_root
        else root / ".local_app_workspace",
        host=args.host,
        port=args.port,
        default_engine=args.default_engine,
        default_fx=args.default_fx,
        default_project=args.default_project,
        default_location=args.default_location,
        default_model=args.default_model,
        default_service_account_key=args.default_service_account_key,
        default_sdk_python=args.default_sdk_python,
        show_advanced_config=args.show_advanced_config,
    )
    try:
        run_server(config)
    except KeyboardInterrupt:
        print("\nstopping expense local app")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
