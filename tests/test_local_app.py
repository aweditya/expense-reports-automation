import importlib.util
import json
import os
import socket
import tempfile
import unittest
import unittest.mock
from pathlib import Path
from subprocess import CompletedProcess


def load_module():
    script_path = Path(__file__).resolve().parent.parent / "scripts" / "local_app.py"
    spec = importlib.util.spec_from_file_location("local_app", script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


local_app = load_module()


def build_manifest(bundle_id: str, *, updated_at_epoch_ms: int = 1000) -> dict:
    return {
        "bundle_id": bundle_id,
        "current_stage": "user_input_required",
        "updated_at_epoch_ms": updated_at_epoch_ms,
        "latest_run_id": "demo_run",
        "documents": [
            {
                "document_id": "doc_receipt",
                "content_sha256": "abc123",
                "original_filenames": ["receipt.png"],
                "stored_filename": "receipt.png",
                "raw_path": "uploads/doc_receipt/receipt.png",
                "media_type": "image/png",
                "byte_count": 42,
                "uploaded_at_epoch_ms": 1000,
                "normalized": {
                    "page_count": 1,
                    "page_image_paths": [],
                    "native_text_path": None,
                },
            }
        ],
        "runs": [
            {
                "run_id": "demo_run",
                "output_dir": "runs/demo_run/artifacts",
                "config": {"engine": "builtin"},
                "filing_status": "UserInputRequired",
                "ledger_state": "UserInputRequired",
            }
        ],
    }


def build_ledger_fixture() -> dict:
    return {
        "summary": {
            "current_state": "user_input_required",
            "current_draft_version_id": 2,
        },
        "draft_versions": [
            {
                "version_id": 1,
                "review_packet": {"summary": {"filing_status": "user_input_required"}},
                "readiness": {
                    "issues": [
                        {"class": "manual_review"},
                        {"class": "user_input_required"},
                    ]
                },
            },
            {
                "version_id": 2,
                "review_packet": {"summary": {"filing_status": "ready_to_file"}},
                "readiness": {
                    "issues": [
                        {"class": "user_input_required"},
                        {"class": "other_warning"},
                    ]
                },
            },
        ],
    }


def write_bundle_fixture(
    workspace_root: Path,
    bundle_id: str,
    *,
    with_workbench: bool = True,
    with_ocr_diff: bool = False,
    with_ocr_grounding: bool = False,
    with_ocr_inspection: bool = False,
) -> None:
    bundle_root = workspace_root / "bundles" / bundle_id
    artifacts_dir = bundle_root / "runs" / "demo_run" / "artifacts"
    uploads_dir = bundle_root / "uploads" / "doc_receipt"
    artifacts_dir.mkdir(parents=True, exist_ok=True)
    uploads_dir.mkdir(parents=True, exist_ok=True)
    (uploads_dir / "receipt.png").write_bytes(b"fixture-receipt")
    (bundle_root / "bundle_manifest.json").write_text(
        json.dumps(build_manifest(bundle_id), indent=2)
    )
    if with_workbench:
        (artifacts_dir / "review_workbench.html").write_text(
            "<!DOCTYPE html><html><body><h1>Stale Saved Workbench</h1></body></html>"
        )
    (artifacts_dir / "ledger.json").write_text(json.dumps(build_ledger_fixture(), indent=2))
    (artifacts_dir / "draft.yaml").write_text("expense_report:\n  general_information: {}\n")
    (artifacts_dir / "review_packet.json").write_text(
        json.dumps({"summary": {"filing_status": "ready_to_file"}}, indent=2)
    )
    if with_ocr_diff:
        ocr_dir = artifacts_dir / "ocr_pass_comparisons" / "doc_receipt"
        ocr_dir.mkdir(parents=True, exist_ok=True)
        (ocr_dir / "comparison.html").write_text("<html><body>OCR Diff</body></html>")
    if with_ocr_grounding:
        grounding_dir = artifacts_dir / "ocr_grounding" / "doc_receipt"
        grounding_dir.mkdir(parents=True, exist_ok=True)
        (grounding_dir / "grounded_preview.html").write_text(
            "<html><body>Grounded OCR</body></html>"
        )
    if with_ocr_inspection:
        inspection_dir = artifacts_dir / "ocr_inspection" / "doc_receipt"
        inspection_dir.mkdir(parents=True, exist_ok=True)
        (inspection_dir / "inspection.html").write_text(
            "<html><body>OCR Inspection</body></html>"
        )


def build_config(
    repo_root: Path,
    workspace_root: Path,
    *,
    show_advanced_config: bool = False,
    default_engine: str = "builtin",
) -> "local_app.LocalAppConfig":
    return local_app.LocalAppConfig(
        repo_root=repo_root,
        workspace_root=workspace_root,
        host="127.0.0.1",
        port=8765,
        default_engine=default_engine,
        default_fx="demo",
        default_project=None,
        default_location="global",
        default_model="gemini-3-flash-preview",
        default_service_account_key="/abs/path/to/key.json"
        if default_engine == "vertex-gemini-sdk"
        else None,
        default_sdk_python="./.venv/bin/python"
        if default_engine == "vertex-gemini-sdk"
        else None,
        show_advanced_config=show_advanced_config,
    )


class LocalAppTests(unittest.TestCase):
    def test_parse_multipart_request_extracts_fields_and_files(self):
        boundary = "----expense-boundary"
        body = (
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="engine"\r\n\r\n'
            "builtin\r\n"
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="documents"; filename="receipt 1.png"\r\n'
            "Content-Type: image/png\r\n\r\n"
            "PNGDATA\r\n"
            f"--{boundary}--\r\n"
        ).encode("utf-8")

        request = local_app.parse_multipart_request(
            f"multipart/form-data; boundary={boundary}",
            body,
        )

        self.assertEqual(request.fields, {"engine": "builtin"})
        self.assertEqual(len(request.files), 1)
        self.assertEqual(request.files[0].filename, "receipt 1.png")
        self.assertEqual(request.files[0].content, b"PNGDATA")

    def test_save_uploaded_files_deduplicates_colliding_names(self):
        uploads = [
            local_app.UploadedFile(
                filename="receipt 1.png",
                content_type="image/png",
                content=b"first",
            ),
            local_app.UploadedFile(
                filename="receipt 1.png",
                content_type="image/png",
                content=b"second",
            ),
        ]

        with tempfile.TemporaryDirectory() as temp_dir:
            saved_paths = local_app.save_uploaded_files(Path(temp_dir), uploads)

            self.assertEqual([path.name for path in saved_paths], ["receipt_1.png", "receipt_1_2.png"])
            self.assertEqual(saved_paths[0].read_bytes(), b"first")
            self.assertEqual(saved_paths[1].read_bytes(), b"second")

    def test_build_ingest_command_uses_cargo_fallback_for_builtin_engine(self):
        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            input_path = Path(workspace_dir) / "receipt.png"
            input_path.write_bytes(b"stub")
            config = build_config(Path(repo_dir), Path(workspace_dir))

            command = local_app.build_ingest_command(
                config,
                {"engine": "builtin", "fx": "demo"},
                [input_path],
            )

            self.assertEqual(command[:4], ["cargo", "run", "--bin", "ingest_bundle_workspace"])
            self.assertIn("--engine", command)
            self.assertIn("builtin", command)
            self.assertIn("--bundle-id", command)
            self.assertIn("receipt", command)
            self.assertNotIn("--run-id", command)

    def test_build_ingest_command_uses_cargo_runner_for_vertex_fields_even_if_binary_exists(self):
        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            repo_root = Path(repo_dir)
            workspace_root = Path(workspace_dir)
            binary = repo_root / "target" / "debug" / "ingest_bundle_workspace"
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o755)
            input_path = workspace_root / "folio.pdf"
            input_path.write_bytes(b"stub")
            config = build_config(
                repo_root,
                workspace_root,
                default_engine="vertex-gemini-sdk",
                show_advanced_config=True,
            )

            command = local_app.build_ingest_command(
                config,
                {
                    "bundle_id": "Demo Bundle",
                    "run_id": "Gemini Flash",
                    "engine": "vertex-gemini-sdk",
                    "project": "demo-project",
                    "location": "global",
                    "model": "gemini-3-flash-preview",
                    "service_account_key": "/tmp/key.json",
                    "sdk_python": "/tmp/venv/bin/python",
                },
                [input_path],
            )

            self.assertEqual(command[:4], ["cargo", "run", "--bin", "ingest_bundle_workspace"])
            self.assertIn("demo_bundle", command)
            self.assertIn("gemini_flash", command)
            self.assertIn("--project", command)
            self.assertIn("demo-project", command)
            self.assertIn("--service-account-key", command)
            self.assertIn("/tmp/key.json", command)
            self.assertIn("--run-id", command)
            self.assertIn("gemini_flash", command)
            self.assertIn("--compare-receipt-passes", command)

    def test_build_ingest_command_uses_server_side_vertex_defaults(self):
        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            config = build_config(
                Path(repo_dir),
                Path(workspace_dir),
                default_engine="vertex-gemini-sdk",
            )
            input_path = Path(workspace_dir) / "receipt.png"
            input_path.write_bytes(b"stub")

            command = local_app.build_ingest_command(
                config,
                {"bundle_id": "demo_bundle"},
                [input_path],
            )

            self.assertIn("--engine", command)
            self.assertIn("vertex-gemini-sdk", command)
            self.assertIn("--service-account-key", command)
            self.assertIn("/abs/path/to/key.json", command)
            self.assertIn("--sdk-python", command)
            self.assertIn("./.venv/bin/python", command)
            self.assertIn("--compare-receipt-passes", command)

    def test_build_review_save_command_supports_base_version(self):
        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as artifacts_dir:
            command = local_app.build_review_save_command(
                Path(repo_dir),
                Path(artifacts_dir),
                Path(artifacts_dir) / "revision.json",
                base_version_id=7,
            )

            self.assertEqual(
                command[:4],
                ["cargo", "run", "--bin", "apply_review_revision_to_artifacts"],
            )
            self.assertIn("--base-version", command)
            self.assertIn("7", command)

    def test_build_render_surface_command_supports_cargo_fallback(self):
        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as artifacts_dir:
            command = local_app.build_render_surface_command(
                Path(repo_dir),
                Path(artifacts_dir),
                "preview",
            )

            self.assertEqual(
                command[:4],
                ["cargo", "run", "--bin", "render_current_review_surface"],
            )
            self.assertIn("--artifacts-dir", command)
            self.assertIn("--surface", command)
            self.assertIn("preview", command)

    def test_render_current_workbench_html_invokes_renderer_cli(self):
        captured = {}

        def runner(command, cwd):
            captured["command"] = command
            captured["cwd"] = cwd
            return CompletedProcess(
                command,
                0,
                stdout="<!DOCTYPE html><html><body><h1>Dynamic Workbench</h1></body></html>",
                stderr="",
            )

        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            workspace_root = Path(workspace_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)

            rendered = local_app.render_current_workbench_html(
                Path(repo_dir),
                workspace_root,
                "demo_bundle",
                command_runner=runner,
            )

            self.assertIn("Dynamic Workbench", rendered)
            self.assertEqual(captured["cwd"], Path(repo_dir))
            self.assertIn("--artifacts-dir", captured["command"])
            self.assertIn("--surface", captured["command"])
            self.assertIn("fa", captured["command"])

    def test_render_current_review_surface_html_supports_preview(self):
        captured = {}

        def runner(command, cwd):
            captured["command"] = command
            return CompletedProcess(
                command,
                0,
                stdout="<!DOCTYPE html><html><body><h1>Preview Surface</h1></body></html>",
                stderr="",
            )

        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            workspace_root = Path(workspace_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)

            rendered = local_app.render_current_review_surface_html(
                Path(repo_dir),
                workspace_root,
                "demo_bundle",
                "preview",
                command_runner=runner,
            )

            self.assertIn("Preview Surface", rendered)
            self.assertIn("--surface", captured["command"])
            self.assertIn("preview", captured["command"])

    def test_list_bundles_sorts_by_latest_update(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "older_bundle")
            newer_root = workspace_root / "bundles" / "newer_bundle"
            newer_root.mkdir(parents=True, exist_ok=True)
            (newer_root / "bundle_manifest.json").write_text(
                json.dumps(build_manifest("newer_bundle", updated_at_epoch_ms=2000), indent=2)
            )

            bundles = local_app.list_bundles(workspace_root)

            self.assertEqual([bundle.bundle_id for bundle in bundles], ["newer_bundle", "older_bundle"])

    def test_latest_workbench_path_returns_existing_html(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(
                workspace_root,
                "demo_bundle",
                with_workbench=True,
                with_ocr_diff=True,
                with_ocr_grounding=True,
                with_ocr_inspection=True,
            )

            workbench_path = local_app.latest_workbench_path(workspace_root, "demo_bundle")

            self.assertIsNotNone(workbench_path)
            assert workbench_path is not None
            self.assertTrue(workbench_path.name.endswith(".html"))

    def test_handle_upload_submission_invokes_pipeline_runner(self):
        captured = {}

        def runner(command, cwd):
            captured["command"] = command
            captured["cwd"] = cwd
            return CompletedProcess(command, 0, stdout="ok", stderr="")

        request = local_app.UploadRequest(
            fields={"engine": "builtin", "bundle_id": "demo_bundle"},
            files=[
                local_app.UploadedFile(
                    filename="receipt.png",
                    content_type="image/png",
                    content=b"image",
                )
            ],
        )

        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            repo_root = Path(repo_dir)
            config = build_config(repo_root, Path(workspace_dir))
            bundle_id = local_app.handle_upload_submission(
                config,
                request,
                command_runner=runner,
            )

            self.assertEqual(bundle_id, "demo_bundle")
            self.assertEqual(captured["cwd"], repo_root)
            self.assertIn("--bundle-id", captured["command"])
            self.assertIn("demo_bundle", captured["command"])
            staged_inputs = [
                Path(value)
                for value in captured["command"]
                if value.endswith(".png")
            ]
            self.assertEqual(len(staged_inputs), 1)
            self.assertTrue(
                str(staged_inputs[0]).startswith(str(repo_root / ".local_runtime"))
            )

    def test_handle_upload_submission_rejects_empty_upload(self):
        request = local_app.UploadRequest(fields={}, files=[])
        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            with self.assertRaises(local_app.LocalAppError):
                local_app.handle_upload_submission(
                    build_config(Path(repo_dir), Path(workspace_dir)),
                    request,
                )

    def test_handle_upload_submission_rejects_bundle_already_inflight(self):
        request = local_app.UploadRequest(
            fields={"engine": "builtin", "bundle_id": "demo_bundle"},
            files=[
                local_app.UploadedFile(
                    filename="receipt.png",
                    content_type="image/png",
                    content=b"image",
                )
            ],
        )
        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            workspace_root = Path(workspace_dir)
            marker_path = local_app.acquire_bundle_inflight_lock(workspace_root, "demo_bundle")
            try:
                with self.assertRaises(local_app.LocalAppError) as ctx:
                    local_app.handle_upload_submission(
                        build_config(Path(repo_dir), workspace_root),
                        request,
                    )
                self.assertIn("already processing", str(ctx.exception))
            finally:
                local_app.release_bundle_inflight_lock(marker_path)

    def test_handle_upload_submission_clears_inflight_lock_after_failure(self):
        def runner(command, cwd):
            return CompletedProcess(command, 1, stdout="", stderr="boom")

        request = local_app.UploadRequest(
            fields={"engine": "builtin", "bundle_id": "demo_bundle"},
            files=[
                local_app.UploadedFile(
                    filename="receipt.png",
                    content_type="image/png",
                    content=b"image",
                )
            ],
        )

        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            workspace_root = Path(workspace_dir)
            config = build_config(Path(repo_dir), workspace_root)
            with self.assertRaises(local_app.LocalAppError):
                local_app.handle_upload_submission(
                    config,
                    request,
                    command_runner=runner,
                )
            self.assertFalse(local_app.bundle_inflight_marker_path(workspace_root, "demo_bundle").exists())

    def test_render_bundle_page_handles_missing_saved_workbench_when_ledger_exists(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=False)
            config = build_config(Path(temp_dir), workspace_root)

            page = local_app.render_bundle_page(
                config,
                local_app.load_bundle_manifest(workspace_root, "demo_bundle"),
            )

            self.assertIn("Workbench availability:</strong> ready", page)
            self.assertIn("/bundle/demo_bundle/overview", page)
            self.assertIn("/bundle/demo_bundle/preview", page)
            self.assertIn("/bundle/demo_bundle/developer", page)
            self.assertIn("/bundle/demo_bundle/document/doc_receipt/receipt.png", page)
            self.assertIn("/bundle/demo_bundle/review-session", page)
            self.assertIn("/bundle/demo_bundle/artifact/ledger.json", page)

    def test_bundle_document_path_accepts_extractor_style_document_ids(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            bundle_root = workspace_root / "bundles" / "demo_bundle"
            uploads_dir = bundle_root / "uploads" / "x00016469619_cd69f9305a1c"
            uploads_dir.mkdir(parents=True, exist_ok=True)
            receipt_path = uploads_dir / "x00016469619.png"
            receipt_path.write_bytes(b"real-receipt")
            manifest = build_manifest("demo_bundle")
            manifest["documents"] = [
                {
                    "document_id": "x00016469619_cd69f9305a1c",
                    "content_sha256": "cd69f9305a1cfeedface00000000000000000000000000000000000000000000",
                    "original_filenames": ["x00016469619.png"],
                    "stored_filename": "x00016469619.png",
                    "raw_path": "uploads/x00016469619_cd69f9305a1c/x00016469619.png",
                    "media_type": "image/png",
                    "byte_count": 12,
                    "uploaded_at_epoch_ms": 1000,
                    "normalized": {
                        "page_count": 1,
                        "page_image_paths": [],
                        "native_text_path": None,
                    },
                }
            ]
            (bundle_root / "bundle_manifest.json").write_text(
                json.dumps(manifest, indent=2)
            )

            resolved = local_app.bundle_document_path(
                workspace_root,
                "demo_bundle",
                "x00016469619",
                "x00016469619.png",
            )

            self.assertEqual(resolved, receipt_path)

    def test_handle_review_save_submission_updates_manifest_stage(self):
        with tempfile.TemporaryDirectory() as repo_dir, tempfile.TemporaryDirectory() as workspace_dir:
            repo_root = Path(repo_dir)
            workspace_root = Path(workspace_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            captured = {}

            def runner(command, cwd):
                captured["command"] = command
                captured["cwd"] = cwd
                self.assertIn("--base-version", command)
                self.assertIn("2", command)
                self.assertEqual(cwd, Path(repo_dir))
                return CompletedProcess(
                    command,
                    0,
                    stdout=json.dumps(
                        {
                            "version_id": 3,
                            "ledger_state": "ready_to_file",
                            "filing_status": "ready_to_file",
                            "automation_gap_count": 0,
                            "user_input_gap_count": 0,
                            "manual_review_count": 0,
                            "other_warning_count": 0,
                            "issue_count": 0,
                        }
                    ),
                    stderr="",
                )

            result = local_app.handle_review_save_submission(
                repo_root,
                workspace_root,
                "demo_bundle",
                {
                    "base_version_id": 2,
                    "actor_role": "financial_administrator",
                    "label": "FA saved revision",
                    "field_edits": [],
                    "confirmed_review_paths": [],
                    "annotations": [],
                },
                command_runner=runner,
            )

            self.assertEqual(result["version_id"], 3)
            revision_json = Path(captured["command"][captured["command"].index("--revision-json") + 1])
            self.assertTrue(
                str(revision_json).startswith(str(repo_root / ".local_runtime"))
            )
            manifest = local_app.load_bundle_manifest(workspace_root, "demo_bundle")
            self.assertEqual(manifest["current_stage"], "ready_to_file")
            self.assertEqual(manifest["runs"][0]["filing_status"], "ready_to_file")

    def test_render_index_page_includes_pending_upload_accumulator(self):
        config = build_config(Path("/tmp/repo"), Path("/tmp/workspace"))

        page = local_app.render_index_page(config, [])

        self.assertIn('id="upload-form"', page)
        self.assertIn('id="documents-input"', page)
        self.assertIn('id="pending-documents"', page)
        self.assertIn("pendingFiles", page)
        self.assertIn("DataTransfer()", page)
        self.assertNotIn("Service Account Key", page)
        self.assertIn("Configured ingestion", page)

    def test_render_index_page_can_show_advanced_config_overrides(self):
        config = build_config(
            Path("/tmp/repo"),
            Path("/tmp/workspace"),
            show_advanced_config=True,
            default_engine="vertex-gemini-sdk",
        )

        page = local_app.render_index_page(config, [])

        self.assertIn("Technical overrides", page)
        self.assertIn("Service Account Key", page)

    def test_ensure_safe_bundle_id_rejects_path_traversal(self):
        with self.assertRaises(local_app.LocalAppError):
            local_app.ensure_safe_bundle_id("../escape")

    def test_main_handles_keyboard_interrupt_cleanly(self):
        original_parse_args = local_app.parse_args
        original_run_server = local_app.run_server

        class Args:
            workspace_root = "/tmp/local-app-test"
            host = "127.0.0.1"
            port = 8765
            default_engine = "builtin"
            default_fx = "demo"
            default_project = None
            default_location = "global"
            default_model = "gemini-3-flash-preview"
            default_service_account_key = None
            default_sdk_python = None
            show_advanced_config = False

        local_app.parse_args = lambda: Args()
        local_app.run_server = lambda config: (_ for _ in ()).throw(KeyboardInterrupt())
        try:
            self.assertEqual(local_app.main(), 0)
        finally:
            local_app.parse_args = original_parse_args
            local_app.run_server = original_run_server


class LocalAppHttpTests(unittest.TestCase):
    def make_config(self, workspace_root: Path):
        config = build_config(Path(__file__).resolve().parent.parent, workspace_root)
        config.port = 0
        return config

    def request(
        self,
        config,
        method: str,
        path: str,
        *,
        body: bytes | None = None,
        headers: dict | None = None,
    ):
        headers = headers or {}
        body = body or b""
        request_lines = [
            f"{method} {path} HTTP/1.1",
            "Host: localhost",
        ]
        for key, value in headers.items():
            request_lines.append(f"{key}: {value}")
        if body and "Content-Length" not in headers:
            request_lines.append(f"Content-Length: {len(body)}")
        request_bytes = ("\r\n".join(request_lines) + "\r\n\r\n").encode("utf-8") + body

        client_sock, server_sock = socket.socketpair()
        try:
            server = type("Server", (), {"config": config})()
            client_sock.sendall(request_bytes)
            client_sock.shutdown(socket.SHUT_WR)
            local_app.LocalAppHandler(server_sock, ("127.0.0.1", 12345), server)
            server_sock.close()
            client_sock.settimeout(1)

            response_bytes = b""
            while True:
                try:
                    chunk = client_sock.recv(65536)
                except TimeoutError:
                    break
                if not chunk:
                    break
                response_bytes += chunk
        finally:
            client_sock.close()
            if server_sock.fileno() != -1:
                server_sock.close()

        header_bytes, payload = response_bytes.split(b"\r\n\r\n", 1)
        header_lines = header_bytes.split(b"\r\n")
        status = int(header_lines[0].split()[1])
        parsed_headers = {}
        for line in header_lines[1:]:
            key, value = line.decode("utf-8").split(":", 1)
            parsed_headers[key.strip()] = value.strip()
        return status, parsed_headers, payload

    def test_http_routes_serve_index_bundle_manifest_and_workbench(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(
                workspace_root,
                "demo_bundle",
                with_workbench=True,
                with_ocr_diff=True,
                with_ocr_grounding=True,
                with_ocr_inspection=True,
            )
            config = self.make_config(workspace_root)
            original_render = local_app.render_current_review_surface_html
            local_app.render_current_review_surface_html = (
                lambda repo_root, workspace_root, bundle_id, surface, command_runner=local_app.run_pipeline_command: (
                    "<!DOCTYPE html><html><body><h1>Dynamic Preview</h1></body></html>"
                    if surface == "preview"
                    else (
                        "<!DOCTYPE html><html><body><h1>Dynamic Developer Workbench</h1><input class='field-control'></body></html>"
                        if surface == "developer"
                        else "<!DOCTYPE html><html><body><h1>Dynamic FA Workbench</h1><input class='field-control'></body></html>"
                    )
                )
            )

            try:
                status, response_headers, payload = self.request(config, "GET", "/")
                self.assertEqual(status, 200)
                self.assertEqual(
                    response_headers["Cache-Control"],
                    "no-store, no-cache, must-revalidate, max-age=0",
                )
                self.assertIn(b"Local FA Intake App", payload)

                status, response_headers, payload = self.request(config, "GET", "/bundle/demo_bundle")
                self.assertEqual(status, 303)
                self.assertEqual(response_headers["Location"], "/bundle/demo_bundle/workbench")
                self.assertEqual(payload, b"")

                status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/overview")
                self.assertEqual(status, 200)
                self.assertIn(b"Managed bundle", payload)

                status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/manifest")
                self.assertEqual(status, 200)
                manifest = json.loads(payload)
                self.assertEqual(manifest["bundle_id"], "demo_bundle")

                status, response_headers, payload = self.request(
                    config, "GET", "/bundle/demo_bundle/workbench"
                )
                self.assertEqual(status, 200)
                self.assertEqual(
                    response_headers["Cache-Control"],
                    "no-store, no-cache, must-revalidate, max-age=0",
                )
                self.assertIn(b"Dynamic FA Workbench", payload)
                self.assertNotIn(b"Stale Saved Workbench", payload)

                status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/preview")
                self.assertEqual(status, 200)
                self.assertIn(b"Dynamic Preview", payload)

                status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/developer")
                self.assertEqual(status, 200)
                self.assertIn(b"Dynamic Developer Workbench", payload)

                status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/review-session")
                self.assertEqual(status, 200)
                review_session = json.loads(payload)
                self.assertEqual(review_session["current_draft_version_id"], 2)

                status, response_headers, payload = self.request(
                    config, "GET", "/bundle/demo_bundle/artifact/ledger.json"
                )
                self.assertEqual(status, 200)
                self.assertEqual(response_headers["Content-Type"], "application/json")
                self.assertEqual(json.loads(payload)["summary"]["current_draft_version_id"], 2)

                status, _, payload = self.request(
                    config, "GET", "/bundle/demo_bundle/artifact/review_workbench.html"
                )
                self.assertEqual(status, 200)
                self.assertIn(b"Dynamic Developer Workbench", payload)

                status, _, payload = self.request(
                    config,
                    "GET",
                    "/bundle/demo_bundle/artifact/ocr_pass_comparisons/doc_receipt/comparison.html",
                )
                self.assertEqual(status, 200)
                self.assertIn(b"OCR Diff", payload)

                status, _, payload = self.request(
                    config,
                    "GET",
                    "/bundle/demo_bundle/artifact/ocr_grounding/doc_receipt/grounded_preview.html",
                )
                self.assertEqual(status, 200)
                self.assertIn(b"Grounded OCR", payload)

                status, _, payload = self.request(
                    config,
                    "GET",
                    "/bundle/demo_bundle/artifact/ocr_inspection/doc_receipt/inspection.html",
                )
                self.assertEqual(status, 200)
                self.assertIn(b"OCR Inspection", payload)

                status, response_headers, payload = self.request(
                    config, "GET", "/bundle/demo_bundle/document/doc_receipt/receipt.png"
                )
                self.assertEqual(status, 200)
                self.assertEqual(response_headers["Content-Type"], "image/png")
                self.assertEqual(payload, b"fixture-receipt")
            finally:
                local_app.render_current_review_surface_html = original_render

    def test_http_upload_redirects_after_successful_submission(self):
        original = local_app.handle_upload_submission

        def fake_handle_upload_submission(config, request, command_runner=local_app.run_pipeline_command):
            self.assertEqual(request.fields["engine"], "builtin")
            self.assertEqual(len(request.files), 1)
            return "demo_bundle"

        local_app.handle_upload_submission = fake_handle_upload_submission
        boundary = "----expense-boundary"
        body = (
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="engine"\r\n\r\n'
            "builtin\r\n"
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="documents"; filename="receipt.png"\r\n'
            "Content-Type: image/png\r\n\r\n"
            "PNGDATA\r\n"
            f"--{boundary}--\r\n"
        ).encode("utf-8")

        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            try:
                config = self.make_config(workspace_root)
                status, response_headers, payload = self.request(
                    config,
                    "POST",
                    "/upload",
                    body=body,
                    headers={
                        "Content-Type": f"multipart/form-data; boundary={boundary}",
                        "Content-Length": str(len(body)),
                    },
                )
                self.assertEqual(status, 303)
                self.assertEqual(response_headers["Location"], "/bundle/demo_bundle")
                self.assertEqual(payload, b"")
            finally:
                local_app.handle_upload_submission = original

    def test_http_review_save_returns_json_error(self):
        original = local_app.handle_review_save_submission

        def fake_handle_review_save_submission(*args, **kwargs):
            raise local_app.LocalAppError("bad review payload")

        local_app.handle_review_save_submission = fake_handle_review_save_submission
        try:
            with tempfile.TemporaryDirectory() as temp_dir:
                workspace_root = Path(temp_dir)
                write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
                config = self.make_config(workspace_root)
                body = json.dumps({"field_edits": []}).encode("utf-8")
                status, response_headers, payload = self.request(
                    config,
                    "POST",
                    "/bundle/demo_bundle/review-session/save",
                    body=body,
                    headers={
                        "Content-Type": "application/json",
                        "Content-Length": str(len(body)),
                    },
                )
                self.assertEqual(status, 400)
                self.assertEqual(response_headers["Content-Type"], "application/json; charset=utf-8")
                self.assertEqual(json.loads(payload)["error"], "bad review payload")
        finally:
            local_app.handle_review_save_submission = original


class DeploymentTests(unittest.TestCase):
    """Tests for Cloud Run deployment readiness: binary resolution and env-aware defaults."""

    # -- Binary resolution: shutil.which finds pre-compiled binary --

    def test_resolve_cli_command_uses_precompiled_binary_when_on_path(self):
        with unittest.mock.patch("shutil.which", return_value="/usr/local/bin/ingest_bundle_workspace"):
            result = local_app.resolve_cli_command(Path("/dummy"))
        self.assertEqual(result, ["/usr/local/bin/ingest_bundle_workspace"])

    def test_resolve_review_cli_command_uses_precompiled_binary_when_on_path(self):
        with unittest.mock.patch("shutil.which", return_value="/usr/local/bin/apply_review_revision_to_artifacts"):
            result = local_app.resolve_review_cli_command(Path("/dummy"))
        self.assertEqual(result, ["/usr/local/bin/apply_review_revision_to_artifacts"])

    def test_resolve_review_surface_cli_command_uses_precompiled_binary_when_on_path(self):
        with unittest.mock.patch("shutil.which", return_value="/usr/local/bin/render_current_review_surface"):
            result = local_app.resolve_review_surface_cli_command(Path("/dummy"))
        self.assertEqual(result, ["/usr/local/bin/render_current_review_surface"])

    # -- Binary resolution: cargo fallback when binary not on PATH --

    def test_resolve_cli_command_falls_back_to_cargo_when_binary_not_found(self):
        with unittest.mock.patch("shutil.which", return_value=None):
            result = local_app.resolve_cli_command(Path("/dummy"))
        self.assertEqual(result, ["cargo", "run", "--bin", "ingest_bundle_workspace", "--"])

    def test_resolve_review_cli_command_falls_back_to_cargo_when_binary_not_found(self):
        with unittest.mock.patch("shutil.which", return_value=None):
            result = local_app.resolve_review_cli_command(Path("/dummy"))
        self.assertEqual(result, ["cargo", "run", "--bin", "apply_review_revision_to_artifacts", "--"])

    def test_resolve_review_surface_cli_command_falls_back_to_cargo_when_binary_not_found(self):
        with unittest.mock.patch("shutil.which", return_value=None):
            result = local_app.resolve_review_surface_cli_command(Path("/dummy"))
        self.assertEqual(result, ["cargo", "run", "--bin", "render_current_review_surface", "--"])

    # -- Environment variable defaults --

    def test_default_port_reads_from_port_env_var(self):
        with unittest.mock.patch.dict(os.environ, {"PORT": "9090"}):
            reloaded = load_module()
        self.assertEqual(reloaded.DEFAULT_PORT, 9090)

    def test_default_host_reads_from_host_env_var(self):
        with unittest.mock.patch.dict(os.environ, {"HOST": "0.0.0.0"}):
            reloaded = load_module()
        self.assertEqual(reloaded.DEFAULT_HOST, "0.0.0.0")


if __name__ == "__main__":
    unittest.main()
