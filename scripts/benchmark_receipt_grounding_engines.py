#!/usr/bin/env python3

import argparse
import copy
import importlib.util
import json
import subprocess
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path


DEFAULT_GEMINI_MODEL = "gemini-3-flash-preview"
DEFAULT_GEMINI_LOCATION = "global"
DEFAULT_DOCUMENT_AI_LOCATION = "us"
DEFAULT_LANES = ("gemini", "document_ai", "hybrid_document_ai_geometry")


class CommandError(RuntimeError):
    pass


def load_ocr_eval_module():
    script_path = Path(__file__).resolve().parent / "evaluate_synthetic_ocr_corpus.py"
    spec = importlib.util.spec_from_file_location("evaluate_synthetic_ocr_corpus", script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


ocr_eval = load_ocr_eval_module()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Benchmark receipt OCR/grounding quality across Gemini, Document AI, "
            "and a Gemini-text + Document-AI-geometry hybrid lane."
        )
    )
    parser.add_argument(
        "--service-account-key",
        required=True,
        help="Path to a Google Cloud service-account JSON key",
    )
    parser.add_argument("--project", help="Google Cloud project id")
    parser.add_argument(
        "--corpus-manifest",
        required=True,
        help="Path to a receipt corpus manifest JSON",
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where lane artifacts and reports should be written",
    )
    parser.add_argument(
        "--sdk-python",
        help="Python interpreter to use for OCR helper scripts",
    )
    parser.add_argument(
        "--gemini-location",
        default=DEFAULT_GEMINI_LOCATION,
        help="Vertex Gemini location, default global",
    )
    parser.add_argument(
        "--gemini-model",
        default=DEFAULT_GEMINI_MODEL,
        help="Gemini model id, default gemini-3-flash-preview",
    )
    parser.add_argument(
        "--docai-location",
        default=DEFAULT_DOCUMENT_AI_LOCATION,
        help="Document AI location, default us",
    )
    parser.add_argument("--docai-processor-id", help="Optional Document AI processor id")
    parser.add_argument(
        "--docai-processor-version",
        help="Optional Document AI processor version id",
    )
    parser.add_argument(
        "--lane",
        action="append",
        dest="lanes",
        help=(
            "Benchmark lane to run. Repeat for multiple. "
            "Choices: gemini, document_ai, hybrid_document_ai_geometry. "
            "Defaults to all three."
        ),
    )
    parser.add_argument(
        "--command-timeout-seconds",
        type=int,
        default=120,
        help="Per-transcription command timeout in seconds, default 120",
    )
    parser.add_argument(
        "--max-documents",
        type=int,
        help="Optional cap on how many corpus documents to benchmark",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=1,
        help="How many documents to transcribe concurrently per lane, default 1",
    )
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def default_sdk_python() -> str:
    venv_python = repo_root() / ".venv" / "bin" / "python"
    if venv_python.exists():
        return str(venv_python)
    return "python3"


def read_json(path: Path) -> dict:
    return json.loads(path.read_text())


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2))


def run_command(command: list[str], cwd: Path, timeout_seconds: int) -> None:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            text=True,
            capture_output=True,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        raise CommandError(
            f"command timed out after {timeout_seconds}s: {' '.join(command)}"
        ) from error
    if completed.stdout:
        print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="")
    if completed.returncode != 0:
        raise CommandError(
            f"command failed with exit code {completed.returncode}: {' '.join(command)}\n"
            f"{(completed.stderr or completed.stdout).rstrip()}"
        )


def resolve_lanes(args: argparse.Namespace) -> list[str]:
    lanes = args.lanes or list(DEFAULT_LANES)
    allowed = set(DEFAULT_LANES)
    for lane in lanes:
        if lane not in allowed:
            raise SystemExit(
                f"unsupported lane {lane!r}; expected one of {', '.join(sorted(allowed))}"
            )
    return lanes


def compare_expected_text_fields(expected_fields: dict, transcription_path: Path) -> dict:
    payload = read_json(transcription_path)
    candidates = ocr_eval.all_region_texts(payload)
    if not candidates:
        candidates = [
            line.strip()
            for page in payload.get("pages") or []
            for line in str(page.get("text") or "").splitlines()
            if line.strip()
        ]
    field_results = []
    match_count = 0
    for field_name, expected_value in expected_fields.items():
        if field_name not in ocr_eval.GROUNDABLE_RECEIPT_FIELDS:
            continue
        matched = any(
            ocr_eval.grounding_field_matches(field_name, expected_value, candidate)
            for candidate in candidates
        )
        match_count += int(matched)
        field_results.append(
            {
                "field": field_name,
                "expected": expected_value,
                "matched": matched,
            }
        )
    return {
        "expected_field_count": len(field_results),
        "matched_field_count": match_count,
        "fields": field_results,
    }


def merge_hybrid_transcription(gemini_payload: dict, document_ai_payload: dict) -> dict:
    hybrid = copy.deepcopy(gemini_payload)
    gemini_pages = {
        (page.get("page_number") or index + 1): page
        for index, page in enumerate(hybrid.get("pages") or [])
    }
    document_ai_pages = {
        (page.get("page_number") or index + 1): page
        for index, page in enumerate(document_ai_payload.get("pages") or [])
    }

    merged_pages = []
    for page_number in sorted(set(gemini_pages) | set(document_ai_pages)):
        gemini_page = copy.deepcopy(gemini_pages.get(page_number) or {"page_number": page_number})
        document_ai_page = document_ai_pages.get(page_number) or {}
        gemini_page["dimensions"] = document_ai_page.get("dimensions") or gemini_page.get(
            "dimensions"
        )
        gemini_page["regions"] = copy.deepcopy(document_ai_page.get("regions") or [])
        merged_pages.append(gemini_page)

    metadata = hybrid.setdefault("metadata", {})
    metadata["producer"] = "hybrid_document_ai_geometry"
    metadata["geometry_source"] = "hybrid"
    metadata["geometry_available"] = bool(
        document_ai_payload.get("metadata", {}).get("geometry_available")
    )
    metadata["model"] = "hybrid_document_ai_geometry"
    hybrid["pages"] = merged_pages
    return hybrid


def transcribe_with_gemini(document: dict, output_path: Path, args: argparse.Namespace) -> None:
    command = [
        "cargo",
        "run",
        "--bin",
        "transcribe_document",
        "--",
        "--engine",
        "vertex-gemini-sdk",
        "--service-account-key",
        args.service_account_key,
        "--location",
        args.gemini_location,
        "--model",
        args.gemini_model,
        "--sdk-python",
        args.sdk_python or default_sdk_python(),
        "--format",
        "json",
        "--output",
        str(output_path),
        document["input_path"],
    ]
    if args.project:
        command.extend(["--project", args.project])
    run_command(command, cwd=repo_root(), timeout_seconds=args.command_timeout_seconds)


def transcribe_with_document_ai(document: dict, output_path: Path, args: argparse.Namespace) -> None:
    command = [
        "cargo",
        "run",
        "--bin",
        "transcribe_document",
        "--",
        "--engine",
        "document-ai",
        "--service-account-key",
        args.service_account_key,
        "--location",
        args.docai_location,
        "--sdk-python",
        args.sdk_python or default_sdk_python(),
        "--format",
        "json",
        "--output",
        str(output_path),
    ]
    if args.project:
        command.extend(["--project", args.project])
    if args.docai_processor_id:
        command.extend(["--processor-id", args.docai_processor_id])
    if args.docai_processor_version:
        command.extend(["--processor-version", args.docai_processor_version])
    command.append(document["input_path"])
    run_command(command, cwd=repo_root(), timeout_seconds=args.command_timeout_seconds)


def summarize_lane_results(lane_name: str, document_results: list[dict]) -> dict:
    available_results = [result for result in document_results if not result.get("error")]
    geometry_available = sum(
        int((result.get("grounding") or {}).get("geometry_available", False))
        for result in available_results
    )
    fully_grounded = sum(
        int(
            (result.get("grounding") or {}).get("expected_field_count", 0) > 0
            and (result.get("grounding") or {}).get("expected_field_count", 0)
            == (result.get("grounding") or {}).get("matched_field_count", 0)
        )
        for result in available_results
    )
    content_match_count = sum(
        int((result.get("markdown") or {}).get("content_match", False))
        for result in available_results
    )
    text_expected = sum(
        (result.get("text_fields") or {}).get("expected_field_count", 0)
        for result in available_results
    )
    text_matched = sum(
        (result.get("text_fields") or {}).get("matched_field_count", 0)
        for result in available_results
    )
    grounding_expected = sum(
        (result.get("grounding") or {}).get("expected_field_count", 0)
        for result in available_results
    )
    grounding_matched = sum(
        (result.get("grounding") or {}).get("matched_field_count", 0)
        for result in available_results
    )
    grounding_field_counts: Counter[str] = Counter()
    grounding_field_miss_counts: Counter[str] = Counter()
    text_field_counts: Counter[str] = Counter()
    text_field_miss_counts: Counter[str] = Counter()
    duration_seconds = [
        float(result.get("duration_seconds") or 0.0) for result in document_results
    ]
    for result in available_results:
        for field in (result.get("text_fields") or {}).get("fields") or []:
            text_field_counts[field["field"]] += int(field.get("matched"))
            text_field_miss_counts[field["field"]] += int(not field.get("matched"))
        for field in (result.get("grounding") or {}).get("fields") or []:
            grounding_field_counts[field["field"]] += int(field.get("matched"))
            grounding_field_miss_counts[field["field"]] += int(not field.get("matched"))

    return {
        "lane": lane_name,
        "document_count": len(document_results),
        "available_document_count": len(available_results),
        "failed_document_count": len(document_results) - len(available_results),
        "content_match_count": content_match_count,
        "text_expected_field_count": text_expected,
        "text_matched_field_count": text_matched,
        "grounding_expected_field_count": grounding_expected,
        "grounding_matched_field_count": grounding_matched,
        "grounding_available_document_count": geometry_available,
        "grounding_fully_matched_document_count": fully_grounded,
        "duration_seconds_total": round(sum(duration_seconds), 3),
        "duration_seconds_max": round(max(duration_seconds, default=0.0), 3),
        "duration_seconds_avg": round(
            sum(duration_seconds) / len(duration_seconds), 3
        )
        if duration_seconds
        else 0.0,
        "text_field_match_counts": dict(text_field_counts),
        "text_field_miss_counts": dict(text_field_miss_counts),
        "grounding_field_match_counts": dict(grounding_field_counts),
        "grounding_field_miss_counts": dict(grounding_field_miss_counts),
    }


def render_markdown_report(report: dict) -> str:
    lines = [
        "# Receipt OCR Engine Benchmark",
        "",
        f"- corpus: {report['corpus_name']}",
        f"- documents: {report['document_count']}",
        "",
    ]
    for lane in report["lanes"]:
        summary = lane["summary"]
        lines.extend(
            [
                f"## {summary['lane']}",
                "",
                f"- available docs: {summary['available_document_count']}/{summary['document_count']}",
                f"- content matches: {summary['content_match_count']}/{summary['available_document_count']}",
                f"- text fields matched: {summary['text_matched_field_count']}/{summary['text_expected_field_count']}",
                f"- grounded fields matched: {summary['grounding_matched_field_count']}/{summary['grounding_expected_field_count']}",
                f"- docs with geometry: {summary['grounding_available_document_count']}",
                f"- fully grounded docs: {summary['grounding_fully_matched_document_count']}",
                f"- avg duration (s): {summary['duration_seconds_avg']}",
                f"- max duration (s): {summary['duration_seconds_max']}",
                "",
            ]
        )
        if summary.get("text_field_miss_counts"):
            lines.append("### text field misses")
            lines.append("")
            for field_name, miss_count in sorted(summary["text_field_miss_counts"].items()):
                lines.append(f"- {field_name}: {miss_count}")
            lines.append("")
        if summary.get("grounding_field_miss_counts"):
            lines.append("### grounding field misses")
            lines.append("")
            for field_name, miss_count in sorted(summary["grounding_field_miss_counts"].items()):
                lines.append(f"- {field_name}: {miss_count}")
            lines.append("")
    return "\n".join(lines)


def escape_html(value: str) -> str:
    return (
        value.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def render_html_report(report: dict) -> str:
    html = [
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">",
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
        "<title>Receipt OCR Engine Benchmark</title>",
        "<style>body{font-family:ui-sans-serif,system-ui,sans-serif;margin:24px;background:#f6f1e8;color:#221b16;}table{border-collapse:collapse;width:100%;margin:16px 0;}th,td{border:1px solid #d9c8b3;padding:10px;text-align:left;}th{background:#ede0cf;}section{margin-bottom:32px;}a{color:#8b4d22;text-decoration:none;}</style>",
        "</head><body>",
        "<h1>Receipt OCR Engine Benchmark</h1>",
        f"<p>corpus: {escape_html(report['corpus_name'])} · documents: {report['document_count']}</p>",
    ]
    for lane in report["lanes"]:
        summary = lane["summary"]
        html.extend(
            [
                f"<section><h2>{escape_html(summary['lane'])}</h2>",
                "<table><tbody>",
                f"<tr><th>available docs</th><td>{summary['available_document_count']}/{summary['document_count']}</td></tr>",
                f"<tr><th>content matches</th><td>{summary['content_match_count']}/{summary['available_document_count']}</td></tr>",
                f"<tr><th>text fields matched</th><td>{summary['text_matched_field_count']}/{summary['text_expected_field_count']}</td></tr>",
                f"<tr><th>grounded fields matched</th><td>{summary['grounding_matched_field_count']}/{summary['grounding_expected_field_count']}</td></tr>",
                f"<tr><th>docs with geometry</th><td>{summary['grounding_available_document_count']}</td></tr>",
                f"<tr><th>fully grounded docs</th><td>{summary['grounding_fully_matched_document_count']}</td></tr>",
                f"<tr><th>avg duration (s)</th><td>{summary['duration_seconds_avg']}</td></tr>",
                f"<tr><th>max duration (s)</th><td>{summary['duration_seconds_max']}</td></tr>",
                "</tbody></table>",
                "<table><thead><tr><th>field</th><th>text misses</th><th>grounding misses</th></tr></thead><tbody>",
            ]
        )
        field_names = sorted(
            set(summary.get("text_field_miss_counts", {}))
            | set(summary.get("grounding_field_miss_counts", {}))
        )
        for field_name in field_names:
            html.extend(
                [
                    "<tr>",
                    f"<td>{escape_html(field_name)}</td>",
                    f"<td>{summary.get('text_field_miss_counts', {}).get(field_name, 0)}</td>",
                    f"<td>{summary.get('grounding_field_miss_counts', {}).get(field_name, 0)}</td>",
                    "</tr>",
                ]
            )
        html.extend(
            [
                "</tbody></table>",
                "<table><thead><tr><th>document</th><th>content</th><th>text fields</th><th>grounding</th><th>duration (s)</th><th>artifact</th><th>error</th></tr></thead><tbody>",
            ]
        )
        for document in lane["documents"]:
            artifact_href = document.get("transcription_path")
            artifact_link = (
                f"<a href=\"file://{escape_html(artifact_href)}\" target=\"_blank\" rel=\"noreferrer noopener\">json</a>"
                if artifact_href
                else ""
            )
            html.extend(
                [
                    "<tr>",
                    f"<td>{escape_html(document['document_id'])}</td>",
                    f"<td>{escape_html(render_ratio((document.get('markdown') or {}).get('content_match'), (document.get('markdown') or {}).get('content_match')))}</td>",
                    f"<td>{escape_html(render_count_summary(document.get('text_fields')))}</td>",
                    f"<td>{escape_html(render_count_summary(document.get('grounding')))}</td>",
                    f"<td>{escape_html(str(document.get('duration_seconds', '')))}</td>",
                    f"<td>{artifact_link}</td>",
                    f"<td>{escape_html(document.get('error') or '')}</td>",
                    "</tr>",
                ]
            )
        html.append("</tbody></table></section>")
    html.append("</body></html>")
    return "".join(html)


def render_ratio(numerator, denominator) -> str:
    if numerator is None or denominator is None:
        return ""
    return "yes" if numerator and denominator else "no"


def render_count_summary(payload: dict | None) -> str:
    if not payload:
        return ""
    return f"{payload.get('matched_field_count', 0)}/{payload.get('expected_field_count', 0)}"


def run_lane_document(
    lane_name: str,
    document: dict,
    lane_dir: Path,
    args: argparse.Namespace,
) -> dict:
    lane_dir.mkdir(parents=True, exist_ok=True)
    transcription_path = lane_dir / f"{document['document_id']}.transcribed.json"
    start_time = time.monotonic()
    try:
        if lane_name == "gemini":
            transcribe_with_gemini(document, transcription_path, args)
        elif lane_name == "document_ai":
            transcribe_with_document_ai(document, transcription_path, args)
        elif lane_name == "hybrid_document_ai_geometry":
            gemini_path = lane_dir / f"{document['document_id']}.gemini.json"
            docai_path = lane_dir / f"{document['document_id']}.document_ai.json"
            transcribe_with_gemini(document, gemini_path, args)
            transcribe_with_document_ai(document, docai_path, args)
            hybrid = merge_hybrid_transcription(read_json(gemini_path), read_json(docai_path))
            write_json(transcription_path, hybrid)
        else:
            raise ValueError(f"unsupported lane {lane_name}")
    except CommandError as error:
        return {
            "document_id": document["document_id"],
            "duration_seconds": round(time.monotonic() - start_time, 3),
            "error": str(error),
        }

    markdown = (
        ocr_eval.compare_markdown(
            Path(document["ground_truth_markdown_path"]),
            transcription_path,
        )
        if document.get("ground_truth_markdown_path")
        else None
    )
    text_fields = compare_expected_text_fields(document.get("expected_fields") or {}, transcription_path)
    grounding = ocr_eval.compare_expected_grounding(
        document.get("expected_fields") or {}, transcription_path
    )
    return {
        "document_id": document["document_id"],
        "transcription_path": str(transcription_path),
        "markdown": markdown,
        "text_fields": text_fields,
        "grounding": grounding,
        "duration_seconds": round(time.monotonic() - start_time, 3),
        "error": None,
    }


def benchmark_lanes(corpus_spec: dict, args: argparse.Namespace) -> dict:
    output_dir = Path(args.output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    lanes = []
    all_documents = [document for packet in corpus_spec["packets"] for document in packet["documents"]]
    if args.max_documents is not None:
        all_documents = all_documents[: args.max_documents]
    for lane_name in resolve_lanes(args):
        lane_dir = output_dir / lane_name
        jobs = max(1, int(getattr(args, "jobs", 1)))
        if jobs == 1 or len(all_documents) <= 1:
            document_results = [
                run_lane_document(lane_name, document, lane_dir, args)
                for document in all_documents
            ]
        else:
            document_results = [None] * len(all_documents)
            with ThreadPoolExecutor(max_workers=jobs) as executor:
                future_to_index = {
                    executor.submit(run_lane_document, lane_name, document, lane_dir, args): index
                    for index, document in enumerate(all_documents)
                }
                for future in as_completed(future_to_index):
                    document_results[future_to_index[future]] = future.result()
        lanes.append(
            {
                "lane": lane_name,
                "summary": summarize_lane_results(lane_name, document_results),
                "documents": document_results,
            }
        )
    return {
        "corpus_name": corpus_spec["corpus_name"],
        "document_count": len(all_documents),
        "lanes": lanes,
    }


def main() -> None:
    args = parse_args()
    corpus_spec = ocr_eval.build_manifest_corpus_spec(Path(args.corpus_manifest).resolve())
    report = benchmark_lanes(corpus_spec, args)

    output_dir = Path(args.output_dir).resolve()
    write_json(output_dir / "engine_benchmark.json", report)
    (output_dir / "engine_benchmark.md").write_text(render_markdown_report(report))
    (output_dir / "engine_benchmark.html").write_text(render_html_report(report))


if __name__ == "__main__":
    main()
