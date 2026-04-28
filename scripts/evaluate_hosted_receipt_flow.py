#!/usr/bin/env python3
"""Run hosted end-to-end receipt ingestion checks against a manifest-backed corpus."""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.parse
from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

import scripts.e2e_test as e2e_test


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Upload each receipt in a corpus manifest to the hosted app, then verify that "
            "the OCR + schema projection flow produces a live bundle with a transaction line."
        )
    )
    parser.add_argument("--cloud-run-url", required=True, help="Direct authenticated Cloud Run URL")
    parser.add_argument("--sa-key", required=True, help="Service account key file for Cloud Run auth")
    parser.add_argument("--corpus-manifest", required=True, type=Path, help="Receipt corpus manifest JSON")
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(".local_runtime/hosted_receipt_flow_eval"),
        help="Directory for JSON/Markdown reports",
    )
    parser.add_argument(
        "--bundle-prefix",
        default="hosted-receipt-flow",
        help="Stable prefix for generated bundle IDs",
    )
    parser.add_argument("--limit", type=int, help="Optional max number of documents from the manifest")
    return parser.parse_args()


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def resolve_manifest_document_paths(manifest_path: Path) -> list[dict[str, Any]]:
    manifest_root = manifest_path.parent.resolve()
    corpus_root = manifest_root.parent
    manifest = read_json(manifest_path)
    documents = manifest.get("documents") or []
    resolved = []
    for index, document in enumerate(documents):
        input_path = Path(document["input_path"])
        if not input_path.is_absolute():
            direct = (manifest_root / input_path).resolve()
            corpus_relative = (corpus_root / input_path).resolve()
            if direct.exists():
                input_path = direct
            else:
                input_path = corpus_relative
        resolved.append(
            {
                "index": index,
                "document_id": document["document_id"],
                "source_name": document.get("source_name"),
                "source_url": document.get("source_url"),
                "input_path": input_path,
                "expected_fields": document.get("expected_fields") or {},
                "tags": document.get("tags") or [],
            }
        )
    return resolved


def build_auth_headers(base_url: str, sa_key: str) -> dict[str, str]:
    token = e2e_test.get_cloud_run_token(base_url, sa_key)
    return {"Authorization": f"Bearer {token}"}


def fetch_artifact_text(
    base_url: str,
    bundle_id: str,
    artifact_name: str,
    headers: dict[str, str],
) -> str:
    url = (
        f"{base_url.rstrip('/')}/bundle/{urllib.parse.quote(bundle_id)}"
        f"/artifact/{urllib.parse.quote(artifact_name)}"
    )
    response = e2e_test._get(url, headers=headers)
    if response.status_code != 200:
        raise RuntimeError(f"artifact {artifact_name} returned HTTP {response.status_code}")
    return response.text


def draft_line_summaries(draft_yaml_text: str) -> list[dict[str, Any]]:
    parsed = yaml.safe_load(draft_yaml_text) or {}
    lines = (((parsed.get("expense_report") or {}).get("transaction_lines")) or [])
    summaries = []
    for line in lines:
        common = line.get("common") or {}
        expense_type = scalar_string(((common.get("expense_type") or {}).get("value")))
        line_amount_usd = scalar_string(((common.get("line_amount_usd") or {}).get("value")))
        original_amount = scalar_string(((common.get("original_amount") or {}).get("value")))
        original_currency = scalar_string(((common.get("original_currency") or {}).get("value")))
        date = scalar_string(((common.get("date") or {}).get("value")))
        remarks = scalar_string(((common.get("remarks") or {}).get("value")))
        summaries.append(
            {
                "expense_type": expense_type,
                "date": date,
                "line_amount_usd": line_amount_usd,
                "original_amount": original_amount,
                "original_currency": original_currency,
                "remarks": remarks,
            }
        )
    return summaries


def scalar_string(value: Any) -> str | None:
    if value is None:
        return None
    return str(value)


def classify_hosted_result(
    *,
    filing_status: str,
    readiness: dict[str, Any],
    draft_lines: list[dict[str, Any]],
) -> dict[str, Any]:
    automation_gap_count = int(readiness.get("automation_gap_count") or 0)
    user_input_gap_count = int(readiness.get("user_input_gap_count") or 0)
    manual_review_count = int(readiness.get("manual_review_count") or 0)
    transaction_line_count = len(draft_lines)
    expense_types = [line["expense_type"] for line in draft_lines if line.get("expense_type")]
    projected_core_fields = {
        "date": any(line.get("date") for line in draft_lines),
        "line_amount_usd": any(line.get("line_amount_usd") for line in draft_lines),
        "original_amount": any(line.get("original_amount") for line in draft_lines),
        "remarks": any(line.get("remarks") for line in draft_lines),
    }
    return {
        "transaction_line_count": transaction_line_count,
        "expense_types": expense_types,
        "projected_core_fields": projected_core_fields,
        "automation_gap_count": automation_gap_count,
        "user_input_gap_count": user_input_gap_count,
        "manual_review_count": manual_review_count,
        "hosted_ocr_ok": filing_status != "automation_blocked",
        "schema_projected": transaction_line_count > 0,
        "projected_without_automation_gaps": transaction_line_count > 0 and automation_gap_count == 0,
        "ready_for_fa_completion": (
            filing_status in {"user_input_required", "ready_to_file"}
            and transaction_line_count > 0
            and automation_gap_count == 0
        ),
    }


def evaluate_document(
    *,
    base_url: str,
    headers: dict[str, str],
    document: dict[str, Any],
    bundle_prefix: str,
) -> dict[str, Any]:
    bundle_id = f"{bundle_prefix}_{document['document_id']}"
    started = time.time()
    try:
        created_bundle_id = e2e_test.upload_documents(
            base_url,
            [document["input_path"]],
            bundle_id=bundle_id,
            headers=headers,
        )
        upload_seconds = round(time.time() - started, 1)
        manifest = e2e_test.check_bundle_manifest(base_url, created_bundle_id, headers=headers)
        review_session = e2e_test.check_review_session(base_url, created_bundle_id, headers=headers)
        draft_yaml_text = fetch_artifact_text(
            base_url,
            created_bundle_id,
            "draft.yaml",
            headers=headers,
        )
        draft_lines = draft_line_summaries(draft_yaml_text)
        classification = classify_hosted_result(
            filing_status=review_session.get("filing_status", "unknown"),
            readiness=review_session.get("readiness") or {},
            draft_lines=draft_lines,
        )
        error = None
    except KeyboardInterrupt:
        raise
    except BaseException as exc:
        created_bundle_id = bundle_id
        upload_seconds = round(time.time() - started, 1)
        manifest = {}
        review_session = {}
        draft_lines = []
        classification = classify_hosted_result(
            filing_status="automation_blocked",
            readiness={
                "automation_gap_count": 1,
                "user_input_gap_count": 0,
                "manual_review_count": 0,
            },
            draft_lines=[],
        )
        error = str(exc)

    return {
        "document_id": document["document_id"],
        "input_path": str(document["input_path"]),
        "source_name": document.get("source_name"),
        "source_url": document.get("source_url"),
        "expected_fields": document.get("expected_fields") or {},
        "bundle_id": created_bundle_id,
        "upload_seconds": upload_seconds,
        "filing_status": review_session.get("filing_status"),
        "readiness": review_session.get("readiness") or {},
        "manifest_document_count": len(manifest.get("documents") or []),
        "manifest_run_count": len(manifest.get("runs") or []),
        "draft_lines": draft_lines,
        "classification": classification,
        "error": error,
    }


def render_markdown(results: list[dict[str, Any]], report: dict[str, Any]) -> str:
    lines = [
        f"# Hosted Receipt Flow Report",
        "",
        f"- Corpus: `{report['corpus_name']}`",
        f"- Target: `{report['target']}`",
        f"- Documents evaluated: `{report['document_count']}`",
        f"- Hosted OCR OK: `{report['hosted_ocr_ok_count']}/{report['document_count']}`",
        f"- Schema projected: `{report['schema_projected_count']}/{report['document_count']}`",
        f"- Projected without automation gaps: `{report['projected_without_automation_gap_count']}/{report['document_count']}`",
        "",
        "## Per Document",
        "",
    ]
    for result in results:
        c = result["classification"]
        lines.extend(
            [
                f"### {result['document_id']}",
                f"- Bundle: `{result['bundle_id']}`",
                f"- Filing status: `{result['filing_status']}`",
                f"- Upload time: `{result['upload_seconds']}s`",
                f"- Transaction lines: `{c['transaction_line_count']}`",
                f"- Expense types: `{', '.join(c['expense_types']) if c['expense_types'] else 'none'}`",
                f"- Automation gaps: `{c['automation_gap_count']}`",
                f"- User-input gaps: `{c['user_input_gap_count']}`",
                f"- Manual review: `{c['manual_review_count']}`",
                f"- Hosted OCR OK: `{c['hosted_ocr_ok']}`",
                f"- Schema projected: `{c['schema_projected']}`",
                f"- Ready for FA completion: `{c['ready_for_fa_completion']}`",
                *([f"- Error: `{result['error']}`"] if result.get("error") else []),
                "",
            ]
        )
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    headers = build_auth_headers(args.cloud_run_url, args.sa_key)
    documents = resolve_manifest_document_paths(args.corpus_manifest)
    if args.limit is not None:
        documents = documents[: args.limit]

    args.output_dir.mkdir(parents=True, exist_ok=True)
    results = []
    for document in documents:
        print(f"Evaluating hosted receipt flow for {document['document_id']} ({document['input_path'].name})")
        results.append(
            evaluate_document(
                base_url=args.cloud_run_url,
                headers=headers,
                document=document,
                bundle_prefix=args.bundle_prefix,
            )
        )

    report = {
        "target": args.cloud_run_url,
        "corpus_name": read_json(args.corpus_manifest).get("corpus_name", args.corpus_manifest.stem),
        "document_count": len(results),
        "hosted_ocr_ok_count": sum(1 for item in results if item["classification"]["hosted_ocr_ok"]),
        "schema_projected_count": sum(1 for item in results if item["classification"]["schema_projected"]),
        "projected_without_automation_gap_count": sum(
            1 for item in results if item["classification"]["projected_without_automation_gaps"]
        ),
        "results": results,
    }

    (args.output_dir / "report.json").write_text(json.dumps(report, indent=2))
    (args.output_dir / "report.md").write_text(render_markdown(results, report))
    print(args.output_dir / "report.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
