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


def build_ledger_fixture(*, ready: bool = False) -> dict:
    if ready:
        return {
            "summary": {
                "current_state": "ready_to_file",
                "current_draft_version_id": 2,
            },
            "draft_versions": [
                {
                    "version_id": 1,
                    "review_packet": {
                        "summary": {"filing_status": "user_input_required"},
                        "issues_queue": [{"class": "user_input_required", "label": "Missing", "path": "a", "message": "fill"}],
                    },
                    "readiness": {
                        "issues": [
                            {"class": "user_input_required"},
                        ]
                    },
                },
                {
                    "version_id": 2,
                    "review_packet": {
                        "summary": {"filing_status": "ready_to_file"},
                        "issues_queue": [],
                    },
                    "readiness": {
                        "issues": []
                    },
                },
            ],
        }
    return {
        "summary": {
            "current_state": "user_input_required",
            "current_draft_version_id": 2,
        },
        "draft_versions": [
            {
                "version_id": 1,
                "review_packet": {
                    "summary": {"filing_status": "user_input_required"},
                    "issues_queue": [{"class": "user_input_required", "label": "Missing", "path": "a", "message": "fill"}],
                },
                "readiness": {
                    "issues": [
                        {"class": "manual_review"},
                        {"class": "user_input_required"},
                    ]
                },
            },
            {
                "version_id": 2,
                "review_packet": {
                    "summary": {"filing_status": "ready_to_file"},
                    "issues_queue": [{"class": "user_input_required", "label": "Missing", "path": "b", "message": "fill"}],
                },
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

            self.assertIn("Workbench:</strong> Ready", page)
            self.assertIn("Open Final Preview (not yet available)", page)
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
        self.assertIn('accept=".pdf,.png,.jpg,.jpeg,.txt,.md,.markdown"', page)
        self.assertIn('id="pending-documents"', page)
        self.assertIn("pendingFiles", page)
        self.assertIn("DataTransfer()", page)
        self.assertIn("setProcessingState", page)
        self.assertIn("Processing your documents.", page)
        self.assertNotIn("Service Account Key", page)
        self.assertIn("Accepts PDF, PNG, JPG, and text documents", page)
        self.assertNotIn("Recent Bundles", page)
        self.assertIn("What Happens Next", page)

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
        self.assertIn("Recent Bundles", page)

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


class FriendlyStageLabelTests(unittest.TestCase):
    """Tests for friendly_stage_label() which maps internal stage strings to user-facing text."""

    def test_known_stages(self):
        cases = {
            "automation_blocked": "Action Required",
            "user_input_required": "Needs Your Input",
            "manual_review_required": "Ready for Review",
            "ready_to_file": "Ready to File",
            "submitted": "Submitted",
            "accepted": "Accepted",
            "returned": "Returned",
            "rejected": "Rejected",
        }
        for stage, expected in cases.items():
            with self.subTest(stage=stage):
                self.assertEqual(local_app.friendly_stage_label(stage), expected)

    def test_unknown_stage_falls_back_to_title_case(self):
        self.assertEqual(local_app.friendly_stage_label("some_new_stage"), "Some New Stage")

    def test_empty_string(self):
        self.assertEqual(local_app.friendly_stage_label(""), "")


class SanitizeIdentifierTests(unittest.TestCase):
    """Tests for sanitize_identifier() security hardening."""

    def test_ascii_alphanumeric_passes_through(self):
        self.assertEqual(local_app.sanitize_identifier("hello123"), "hello123")

    def test_unicode_letters_are_stripped(self):
        result = local_app.sanitize_identifier("报告_2024")
        self.assertTrue(result.isascii(), f"Expected ASCII-only, got {result!r}")
        self.assertEqual(result, "2024")

    def test_long_identifier_is_truncated(self):
        long_id = "a" * 500
        result = local_app.sanitize_identifier(long_id)
        self.assertLessEqual(len(result), local_app.MAX_IDENTIFIER_LENGTH)

    def test_path_traversal_stripped(self):
        self.assertEqual(local_app.sanitize_identifier("../../etc/passwd"), "etc_passwd")

    def test_xss_payload_stripped(self):
        self.assertEqual(local_app.sanitize_identifier("<script>alert(1)</script>"), "script_alert_1_script")


class EnsureSafeBundleIdTests(unittest.TestCase):
    """Tests for ensure_safe_bundle_id() validation."""

    def test_rejects_empty(self):
        with self.assertRaises(local_app.LocalAppError):
            local_app.ensure_safe_bundle_id("")

    def test_rejects_too_long(self):
        with self.assertRaises(local_app.LocalAppError) as ctx:
            local_app.ensure_safe_bundle_id("a" * 300)
        self.assertIn("too long", str(ctx.exception))

    def test_truncates_reflected_invalid_id_in_error(self):
        with self.assertRaises(local_app.LocalAppError) as ctx:
            local_app.ensure_safe_bundle_id("bad-name")
        msg = str(ctx.exception)
        self.assertIn("invalid bundle identifier", msg)
        self.assertLessEqual(len(msg), 200)

    def test_accepts_valid_id(self):
        self.assertEqual(local_app.ensure_safe_bundle_id("valid_id_123"), "valid_id_123")


class SanitizePipelineErrorTests(unittest.TestCase):
    """Tests for sanitize_pipeline_error() which strips internals from stderr."""

    def test_strips_cargo_output(self):
        raw = (
            "   Compiling expense_report_schema v0.1.0\n"
            "    Finished `dev` profile [unoptimized + debuginfo]\n"
            "     Running `target/debug/ingest_bundle_workspace`\n"
            "pdfinfo failed: not a PDF file\n"
        )
        result = local_app.sanitize_pipeline_error(raw)
        self.assertNotIn("Compiling", result)
        self.assertNotIn("Finished", result)
        self.assertNotIn("Running `", result)
        self.assertIn("pdfinfo failed", result)

    def test_scrubs_filesystem_paths(self):
        raw = "Error reading /Users/adityasriram/Labs/project/file.pdf"
        result = local_app.sanitize_pipeline_error(raw)
        self.assertNotIn("/Users/adityasriram", result)
        self.assertIn("[path]", result)

    def test_truncates_long_messages(self):
        raw = "error: " + "x" * 1000
        result = local_app.sanitize_pipeline_error(raw)
        self.assertLessEqual(len(result), 510)

    def test_empty_input_gives_default_message(self):
        result = local_app.sanitize_pipeline_error("")
        self.assertIn("could not be processed", result)

    def test_scrubs_tmp_paths(self):
        raw = "failed to read /tmp/stress_test_workspace/bundles/test/uploads/file.pdf"
        result = local_app.sanitize_pipeline_error(raw)
        self.assertNotIn("/tmp/stress_test", result)


class FormatHelperTests(unittest.TestCase):
    """Tests for format_file_size(), pluralize(), format_epoch_ms()."""

    def test_format_file_size_bytes(self):
        self.assertEqual(local_app.format_file_size(500), "500 bytes")

    def test_format_file_size_kb(self):
        self.assertEqual(local_app.format_file_size(2048), "2.0 KB")

    def test_format_file_size_mb(self):
        self.assertEqual(local_app.format_file_size(2_500_000), "2.4 MB")

    def test_pluralize_singular(self):
        self.assertEqual(local_app.pluralize(1, "document"), "1 document")

    def test_pluralize_plural(self):
        self.assertEqual(local_app.pluralize(3, "document"), "3 documents")

    def test_pluralize_zero(self):
        self.assertEqual(local_app.pluralize(0, "item"), "0 items")

    def test_format_epoch_ms(self):
        result = local_app.format_epoch_ms(1714200000000)
        self.assertIn("2024", result)


class ErrorPageTests(unittest.TestCase):
    """Tests for render_error_page() and custom error handling."""

    def test_render_404_page_is_branded(self):
        page = local_app.render_error_page(404, "Not found")
        self.assertIn("Page Not Found", page)
        self.assertIn("Back to upload page", page)
        self.assertNotIn("Error code explanation", page)

    def test_render_405_page(self):
        page = local_app.render_error_page(405, "Method Not Allowed")
        self.assertIn("Method Not Allowed", page)

    def test_error_page_escapes_html(self):
        page = local_app.render_error_page(400, "<script>alert(1)</script>")
        self.assertNotIn("<script>alert(1)</script>", page)
        self.assertIn("&lt;script&gt;", page)


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
                self.assertIn(b"Upload &amp; Process", payload)

                status, response_headers, payload = self.request(config, "GET", "/bundle/demo_bundle")
                self.assertEqual(status, 303)
                self.assertEqual(response_headers["Location"], "/bundle/demo_bundle/workbench")
                self.assertEqual(payload, b"")

                status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/overview")
                self.assertEqual(status, 200)
                self.assertIn(b"Expense Report", payload)

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
                self.assertIn(b"Preview Not Yet Available", payload)

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


    def test_preview_route_serves_preview_when_ready_to_file(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            # Overwrite ledger with ready-to-file state
            artifacts_dir = workspace_root / "bundles" / "demo_bundle" / "runs" / "demo_run" / "artifacts"
            (artifacts_dir / "ledger.json").write_text(
                json.dumps(build_ledger_fixture(ready=True), indent=2)
            )
            config = self.make_config(workspace_root)
            original_render = local_app.render_current_review_surface_html
            local_app.render_current_review_surface_html = (
                lambda repo_root, workspace_root, bundle_id, surface, command_runner=local_app.run_pipeline_command: (
                    "<!DOCTYPE html><html><body><h1>Dynamic Preview</h1></body></html>"
                )
            )
            try:
                status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/preview")
                self.assertEqual(status, 200)
                self.assertIn(b"Dynamic Preview", payload)
                self.assertNotIn(b"Preview Not Yet Available", payload)
            finally:
                local_app.render_current_review_surface_html = original_render

    def test_preview_route_returns_gate_page_when_not_ready_to_file(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            config = self.make_config(workspace_root)

            status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/preview")
            self.assertEqual(status, 200)
            self.assertIn(b"Preview Not Yet Available", payload)
            self.assertIn(b"Needs Your Input", payload)
            self.assertIn(b"Open Workbench", payload)
            self.assertNotIn(b"Dynamic Preview", payload)

    def test_preview_route_keeps_gate_when_session_is_inconsistent(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            config = self.make_config(workspace_root)
            original_load_session = local_app.load_review_session_state
            original_render = local_app.render_current_review_surface_html
            local_app.load_review_session_state = lambda _workspace_root, _bundle_id: {
                "filing_status": "ready_to_file",
                "issue_count": 2,
                "readiness": {
                    "automation_gap_count": 1,
                    "user_input_gap_count": 0,
                    "manual_review_count": 0,
                    "other_warning_count": 0,
                },
            }
            local_app.render_current_review_surface_html = (
                lambda repo_root, workspace_root, bundle_id, surface, command_runner=local_app.run_pipeline_command: (
                    "<!DOCTYPE html><html><body><h1>Dynamic Preview</h1></body></html>"
                )
            )
            try:
                status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/preview")
                self.assertEqual(status, 200)
                self.assertIn(b"Preview Not Yet Available", payload)
                self.assertNotIn(b"Dynamic Preview", payload)
            finally:
                local_app.load_review_session_state = original_load_session
                local_app.render_current_review_surface_html = original_render

    def test_preview_gate_page_shows_readiness_counts(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            session = local_app.load_review_session_state(workspace_root, "demo_bundle")
            config = build_config(Path(temp_dir), workspace_root)

            page = local_app.render_preview_gate_page(config, "demo_bundle", session)

            self.assertIn("Could Not Extract", page)
            self.assertIn("Needs Your Input", page)
            self.assertIn("Review Required", page)
            self.assertIn("/bundle/demo_bundle/workbench", page)
            self.assertIn("/bundle/demo_bundle/overview", page)

    def test_preview_gate_page_links_back_to_workbench(self):
        session = {
            "filing_status": "user_input_required",
            "issue_count": 3,
            "readiness": {
                "automation_gap_count": 0,
                "user_input_gap_count": 2,
                "manual_review_count": 1,
            },
        }
        config = build_config(Path("/tmp/repo"), Path("/tmp/workspace"))

        page = local_app.render_preview_gate_page(config, "test_bundle", session)

        self.assertIn("/bundle/test_bundle/workbench", page)
        self.assertIn("Open Workbench", page)
        self.assertIn("/bundle/test_bundle/overview", page)


    def test_custom_404_page_for_unknown_route(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            config = self.make_config(Path(temp_dir))
            status, _, payload = self.request(config, "GET", "/nonexistent")
            self.assertEqual(status, 404)
            self.assertIn(b"Page Not Found", payload)
            self.assertIn(b"Back to upload page", payload)
            self.assertNotIn(b"Error code explanation", payload)

    def test_post_to_index_returns_405(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            config = self.make_config(Path(temp_dir))
            status, _, payload = self.request(config, "POST", "/")
            self.assertEqual(status, 405)
            self.assertIn(b"Method Not Allowed", payload)

    def test_server_version_header_hides_python_version(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            config = self.make_config(Path(temp_dir))
            _, headers, _ = self.request(config, "GET", "/")
            server = headers.get("Server", "")
            self.assertNotIn("Python", server)
            self.assertIn("ExpenseLocalApp", server)

    def test_head_request_returns_headers_without_body(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            config = self.make_config(Path(temp_dir))
            status, headers, payload = self.request(config, "HEAD", "/")
            self.assertEqual(status, 200)
            self.assertEqual(payload, b"")

    def test_debug_metrics_endpoint(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            config = self.make_config(workspace_root)
            status, headers, payload = self.request(config, "GET", "/debug/metrics")
            self.assertEqual(status, 200)
            self.assertEqual(headers["Content-Type"], "application/json; charset=utf-8")
            data = json.loads(payload)
            self.assertEqual(data["bundle_count"], 1)
            self.assertIn("stages", data)
            self.assertIn("workspace_size_human", data)

    def test_debug_bundles_endpoint(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            config = self.make_config(workspace_root)
            status, _, payload = self.request(config, "GET", "/debug/bundles")
            self.assertEqual(status, 200)
            data = json.loads(payload)
            self.assertIsInstance(data, list)
            self.assertEqual(len(data), 1)
            self.assertEqual(data[0]["bundle_id"], "demo_bundle")
            self.assertIn("review_session", data[0])

    def test_debug_unknown_returns_404(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            config = self.make_config(Path(temp_dir))
            status, _, _ = self.request(config, "GET", "/debug/unknown")
            self.assertEqual(status, 404)

    def test_bundle_overview_shows_file_sizes_formatted(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            config = self.make_config(workspace_root)
            status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/overview")
            self.assertEqual(status, 200)
            self.assertIn(b"42 bytes", payload)
            self.assertNotIn(b"image/png", payload)

    def test_bundle_overview_shows_formatted_timestamp(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            config = self.make_config(workspace_root)
            status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/overview")
            self.assertEqual(status, 200)
            self.assertIn(b"Last processed:", payload)
            self.assertNotIn(b"demo_run", payload)

    def test_bundle_overview_uses_check_for_updates_button(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle", with_workbench=True)
            config = self.make_config(workspace_root)
            status, _, payload = self.request(config, "GET", "/bundle/demo_bundle/overview")
            self.assertIn(b"Check for Updates", payload)
            self.assertNotIn(b"Refresh This Page", payload)


class PreviewGateTests(unittest.TestCase):
    """Tests for the preview gate logic and bundle overview link styling."""

    def test_filing_status_from_counts_returns_automation_blocked(self):
        self.assertEqual(
            local_app.filing_status_from_counts({
                "automation_gap_count": 1,
                "user_input_gap_count": 0,
                "manual_review_count": 0,
            }),
            "automation_blocked",
        )

    def test_filing_status_from_counts_returns_user_input_required(self):
        self.assertEqual(
            local_app.filing_status_from_counts({
                "automation_gap_count": 0,
                "user_input_gap_count": 2,
                "manual_review_count": 0,
            }),
            "user_input_required",
        )

    def test_filing_status_from_counts_returns_manual_review_required(self):
        self.assertEqual(
            local_app.filing_status_from_counts({
                "automation_gap_count": 0,
                "user_input_gap_count": 0,
                "manual_review_count": 1,
            }),
            "manual_review_required",
        )

    def test_filing_status_from_counts_returns_ready_to_file(self):
        self.assertEqual(
            local_app.filing_status_from_counts({
                "automation_gap_count": 0,
                "user_input_gap_count": 0,
                "manual_review_count": 0,
            }),
            "ready_to_file",
        )

    def test_filing_status_priority_automation_over_user_input(self):
        self.assertEqual(
            local_app.filing_status_from_counts({
                "automation_gap_count": 1,
                "user_input_gap_count": 3,
                "manual_review_count": 2,
            }),
            "automation_blocked",
        )

    def test_filing_status_priority_user_input_over_manual_review(self):
        self.assertEqual(
            local_app.filing_status_from_counts({
                "automation_gap_count": 0,
                "user_input_gap_count": 1,
                "manual_review_count": 5,
            }),
            "user_input_required",
        )

    def test_preview_is_available_requires_ready_status_and_no_remaining_items(self):
        self.assertTrue(
            local_app.preview_is_available(
                {
                    "filing_status": "ready_to_file",
                    "issue_count": 0,
                    "readiness": {
                        "automation_gap_count": 0,
                        "user_input_gap_count": 0,
                        "manual_review_count": 0,
                    },
                }
            )
        )
        self.assertFalse(
            local_app.preview_is_available(
                {
                    "filing_status": "ready_to_file",
                    "issue_count": 1,
                    "readiness": {
                        "automation_gap_count": 0,
                        "user_input_gap_count": 0,
                        "manual_review_count": 0,
                    },
                }
            )
        )
        self.assertFalse(
            local_app.preview_is_available(
                {
                    "filing_status": "automation_blocked",
                    "issue_count": 0,
                    "readiness": {
                        "automation_gap_count": 0,
                        "user_input_gap_count": 0,
                        "manual_review_count": 0,
                    },
                }
            )
        )

    def test_load_review_session_state_computes_correct_readiness(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle")

            session = local_app.load_review_session_state(workspace_root, "demo_bundle")

            self.assertEqual(session["current_draft_version_id"], 2)
            self.assertEqual(session["filing_status"], "user_input_required")
            self.assertEqual(session["readiness"]["user_input_gap_count"], 1)
            self.assertEqual(session["readiness"]["other_warning_count"], 1)
            self.assertEqual(session["readiness"]["automation_gap_count"], 0)
            self.assertEqual(session["readiness"]["manual_review_count"], 0)

    def test_load_review_session_state_ready_to_file(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle")
            artifacts_dir = workspace_root / "bundles" / "demo_bundle" / "runs" / "demo_run" / "artifacts"
            (artifacts_dir / "ledger.json").write_text(
                json.dumps(build_ledger_fixture(ready=True), indent=2)
            )

            session = local_app.load_review_session_state(workspace_root, "demo_bundle")

            self.assertEqual(session["filing_status"], "ready_to_file")
            self.assertEqual(session["readiness"]["user_input_gap_count"], 0)
            self.assertEqual(session["readiness"]["manual_review_count"], 0)
            self.assertEqual(session["issue_count"], 0)

    def test_bundle_overview_shows_muted_preview_link_when_not_ready(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle")
            config = build_config(Path(temp_dir), workspace_root)
            manifest = local_app.load_bundle_manifest(workspace_root, "demo_bundle")

            page = local_app.render_bundle_page(config, manifest)

            self.assertIn("not yet available", page)
            self.assertIn('opacity: 0.5', page)

    def test_bundle_overview_shows_active_preview_link_when_ready(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_root = Path(temp_dir)
            write_bundle_fixture(workspace_root, "demo_bundle")
            # Update manifest and ledger to reflect a fully ready packet
            manifest_path = workspace_root / "bundles" / "demo_bundle" / "bundle_manifest.json"
            manifest = json.loads(manifest_path.read_text())
            manifest["runs"][0]["filing_status"] = "ready_to_file"
            manifest_path.write_text(json.dumps(manifest, indent=2))
            artifacts_dir = workspace_root / "bundles" / "demo_bundle" / "runs" / "demo_run" / "artifacts"
            (artifacts_dir / "ledger.json").write_text(
                json.dumps(build_ledger_fixture(ready=True), indent=2)
            )
            config = build_config(Path(temp_dir), workspace_root)
            manifest = local_app.load_bundle_manifest(workspace_root, "demo_bundle")

            page = local_app.render_bundle_page(config, manifest)

            self.assertIn("Open Final Preview", page)
            self.assertNotIn("not yet available", page)


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
