#!/usr/bin/env python3

import argparse
import json
import os
import subprocess
from collections import Counter
from pathlib import Path


DEFAULT_MODELS = ["gemini-3-flash-preview", "gemini-3-pro-preview"]
DEFAULT_LOCATION = "global"
DEFAULT_PACKETS = 4


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run end-to-end OCR ingestion on a rendered synthetic corpus, score "
            "markdown fidelity against the source markdown, and compare Gemini 3 models."
        )
    )
    parser.add_argument(
        "--service-account-key",
        required=True,
        help="Path to a Google Cloud service-account JSON key",
    )
    parser.add_argument("--project", help="Google Cloud project id")
    parser.add_argument(
        "--location",
        default=DEFAULT_LOCATION,
        help="Vertex AI location; default is global",
    )
    parser.add_argument(
        "--model",
        action="append",
        dest="models",
        help=(
            "Gemini 3 model id to use for OCR. Repeat the flag to compare multiple models. "
            "Defaults to gemini-3-flash-preview and gemini-3-pro-preview."
        ),
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where generated corpus, rendered docs, and reports should be written",
    )
    parser.add_argument(
        "--packets",
        type=int,
        default=DEFAULT_PACKETS,
        help="Number of synthetic packets to evaluate",
    )
    parser.add_argument(
        "--sdk-python",
        help="Python interpreter to use for the Gemini SDK helper path",
    )
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def default_sdk_python() -> str:
    env_value = os.environ.get("VERTEX_GEMINI_SDK_PYTHON")
    if env_value:
        return env_value
    venv_python = Path("/tmp/expense_report_genai_venv/bin/python")
    if venv_python.exists():
        return str(venv_python)
    return "python3"


def resolve_models(args: argparse.Namespace) -> list[str]:
    return args.models or list(DEFAULT_MODELS)


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
    identifier = "".join(cleaned).strip("_")
    return identifier or "model"


class CommandError(RuntimeError):
    pass


def run_command(command: list[str], cwd: Path) -> None:
    completed = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    if completed.stdout:
        print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="")
    if completed.returncode != 0:
        raise CommandError(
            f"command failed with exit code {completed.returncode}: {' '.join(command)}\n"
            f"{(completed.stderr or completed.stdout).rstrip()}"
        )


def read_json(path: Path) -> dict:
    return json.loads(path.read_text())


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2))


def normalize_markdown(text: str) -> str:
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").split("\n")]
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def relax_markdown(text: str) -> str:
    relaxed_lines = []
    previous_blank = False
    for line in normalize_markdown(text).split("\n"):
        is_blank = not line
        if is_blank and previous_blank:
            continue
        relaxed_lines.append(line)
        previous_blank = is_blank
    return "\n".join(relaxed_lines)


def content_only_markdown(text: str) -> str:
    return "\n".join(
        line for line in normalize_markdown(text).split("\n") if line.strip()
    )


def join_transcribed_pages(payload: dict) -> str:
    return "\n\n".join(page["text"] for page in payload.get("pages") or [])


def compare_markdown(source_path: Path, transcribed_path: Path) -> dict:
    source_text = normalize_markdown(source_path.read_text())
    transcribed_text = normalize_markdown(join_transcribed_pages(read_json(transcribed_path)))
    return {
        "exact_match": source_text == transcribed_text,
        "relaxed_match": relax_markdown(source_text) == relax_markdown(transcribed_text),
        "content_match": content_only_markdown(source_text)
        == content_only_markdown(transcribed_text),
    }


def summarize_readiness(readiness_payload: dict) -> dict:
    counts = Counter(issue["class"] for issue in readiness_payload.get("issues") or [])
    return {
        "automation_gap_count": counts["automation_gap"],
        "user_input_gap_count": counts["user_input_required"],
        "manual_review_item_count": counts["manual_review"],
        "other_warning_count": counts["other_warning"],
    }


def render_document(source_path: Path, output_dir: Path, document_kind: str) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    render_format = "pdf" if document_kind == "hotel_folio" else "png"
    run_command(
        [
            "python3",
            "scripts/render_text_documents_for_ocr.py",
            "--output-dir",
            str(output_dir),
            "--format",
            render_format,
            str(source_path),
        ],
        cwd=repo_root(),
    )
    suffix = ".pdf" if render_format == "pdf" else ".png"
    return output_dir.joinpath(f"{source_path.stem}{suffix}")


def prepare_source_corpus(source_dir: Path, packet_count: int) -> dict:
    run_command(
        [
            "cargo",
            "run",
            "--bin",
            "generate_synthetic_corpus",
            "--",
            "--output-dir",
            str(source_dir),
            "--packets",
            str(packet_count),
        ],
        cwd=repo_root(),
    )
    return read_json(source_dir / "manifest.json")


def render_corpus_documents(manifest: dict, source_dir: Path, rendered_dir: Path) -> dict[str, list[Path]]:
    rendered_paths_by_packet = {}

    for packet in manifest["packets"]:
        packet_id = packet["packet_id"]
        packet_source_dir = source_dir / packet_id
        packet_rendered_dir = rendered_dir / packet_id
        rendered_paths = []

        for document in packet["documents"]:
            source_path = packet_source_dir / document["filename"]
            rendered_paths.append(
                render_document(source_path, packet_rendered_dir, document["kind"])
            )

        rendered_paths_by_packet[packet_id] = rendered_paths

    return rendered_paths_by_packet


def ingest_packet(
    packet_id: str,
    rendered_paths: list[Path],
    output_dir: Path,
    args: argparse.Namespace,
    model: str,
) -> None:
    command = [
        "cargo",
        "run",
        "--bin",
        "ingest_expense_documents",
        "--",
        "--output-dir",
        str(output_dir),
        "--bundle-id",
        packet_id,
        "--fx",
        "demo",
        "--engine",
        "vertex-gemini-sdk",
        "--service-account-key",
        args.service_account_key,
        "--location",
        args.location,
        "--model",
        model,
        "--sdk-python",
        args.sdk_python or default_sdk_python(),
    ]
    if args.project:
        command.extend(["--project", args.project])
    command.extend(str(path) for path in rendered_paths)
    run_command(command, cwd=repo_root())


def evaluate_model(
    manifest: dict,
    source_dir: Path,
    ingestion_root: Path,
    rendered_paths_by_packet: dict[str, list[Path]],
    args: argparse.Namespace,
    model: str,
) -> dict:
    packet_results = []
    exact_match_count = 0
    relaxed_match_count = 0
    content_match_count = 0
    filing_status_counts: Counter[str] = Counter()
    ledger_state_counts: Counter[str] = Counter()
    document_count = 0
    model_key = sanitize_identifier(model)

    for packet in manifest["packets"]:
        packet_id = packet["packet_id"]
        packet_source_dir = source_dir / packet_id
        packet_ingestion_dir = ingestion_root / model_key / packet_id
        packet_ingestion_dir.mkdir(parents=True, exist_ok=True)

        ingest_packet(
            packet_id,
            rendered_paths_by_packet[packet_id],
            packet_ingestion_dir,
            args,
            model,
        )

        readiness = summarize_readiness(read_json(packet_ingestion_dir / "readiness.json"))
        manifest_entry = read_json(packet_ingestion_dir / "manifest.json")
        ledger = read_json(packet_ingestion_dir / "ledger.json")

        packet_document_results = []
        for document in packet["documents"]:
            source_path = packet_source_dir / document["filename"]
            transcription_path = (
                packet_ingestion_dir
                / "transcriptions"
                / f"{source_path.stem}.transcribed.json"
            )
            comparison = compare_markdown(source_path, transcription_path)
            exact_match_count += int(comparison["exact_match"])
            relaxed_match_count += int(comparison["relaxed_match"])
            content_match_count += int(comparison["content_match"])
            document_count += 1

            rendered_path = next(
                path
                for path in rendered_paths_by_packet[packet_id]
                if path.stem == source_path.stem
            )
            packet_document_results.append(
                {
                    "kind": document["kind"],
                    "source_markdown": str(source_path),
                    "rendered_document": str(rendered_path),
                    "transcription_json": str(transcription_path),
                    **comparison,
                }
            )

        filing_status = manifest_entry["filing_status"]
        ledger_state = manifest_entry["ledger_state"]
        filing_status_counts[filing_status] += 1
        ledger_state_counts[ledger_state] += 1

        packet_results.append(
            {
                "packet_id": packet_id,
                "filing_status": filing_status,
                "ledger_state": ledger_state,
                "readiness": readiness,
                "documents": packet_document_results,
                "draft_version_count": ledger["summary"]["draft_version_count"],
                "submission_attempt_count": ledger["summary"]["submission_attempt_count"],
            }
        )

    return {
        "summary": {
            "packet_count": len(packet_results),
            "document_count": document_count,
            "model": model,
            "model_key": model_key,
            "location": args.location,
            "exact_match_count": exact_match_count,
            "relaxed_match_count": relaxed_match_count,
            "content_match_count": content_match_count,
            "filing_status_counts": dict(filing_status_counts),
            "ledger_state_counts": dict(ledger_state_counts),
        },
        "packets": packet_results,
    }


def summarize_comparison(model_reports: list[dict]) -> dict:
    return {
        "models": [
            {
                "model": report["summary"]["model"],
                "model_key": report["summary"]["model_key"],
                "status": report["summary"].get("status", "ok"),
                "error": report["summary"].get("error"),
                "error_summary": report["summary"].get("error_summary")
                or (
                    condense_error_message(report["summary"]["error"])
                    if report["summary"].get("error")
                    else None
                ),
                "packet_count": report["summary"]["packet_count"],
                "document_count": report["summary"]["document_count"],
                "exact_match_count": report["summary"]["exact_match_count"],
                "relaxed_match_count": report["summary"]["relaxed_match_count"],
                "content_match_count": report["summary"]["content_match_count"],
                "filing_status_counts": report["summary"]["filing_status_counts"],
                "ledger_state_counts": report["summary"]["ledger_state_counts"],
            }
            for report in model_reports
        ]
    }


def condense_error_message(error: str) -> str:
    if "404 NOT_FOUND" in error:
        return "404 NOT_FOUND"
    first_line = next((line.strip() for line in error.splitlines() if line.strip()), "")
    return first_line or "command failed"


def failed_model_report(model: str, location: str, error: str) -> dict:
    return {
        "summary": {
            "model": model,
            "model_key": sanitize_identifier(model),
            "location": location,
            "status": "error",
            "error": error,
            "error_summary": condense_error_message(error),
            "packet_count": 0,
            "document_count": 0,
            "exact_match_count": 0,
            "relaxed_match_count": 0,
            "content_match_count": 0,
            "filing_status_counts": {},
            "ledger_state_counts": {},
        },
        "packets": [],
    }


def render_model_report_markdown(report: dict) -> str:
    lines = [
        "# Synthetic OCR Evaluation",
        "",
        f"- status: {report['summary'].get('status', 'ok')}",
        f"- packets: {report['summary']['packet_count']}",
        f"- documents: {report['summary']['document_count']}",
        f"- model: {report['summary']['model']}",
        f"- exact markdown matches: {report['summary']['exact_match_count']}",
        f"- relaxed markdown matches: {report['summary']['relaxed_match_count']}",
        f"- content-only matches: {report['summary']['content_match_count']}",
    ]
    if report["summary"].get("error"):
        lines.extend(["", "## Error", f"- {report['summary']['error']}"])
        return "\n".join(lines) + "\n"

    lines.extend(["", "## Filing Status Counts"])
    for key, value in sorted(report["summary"]["filing_status_counts"].items()):
        lines.append(f"- {key}: {value}")
    lines.append("")
    lines.append("## Ledger State Counts")
    for key, value in sorted(report["summary"]["ledger_state_counts"].items()):
        lines.append(f"- {key}: {value}")
    lines.append("")
    lines.append("## Packet Results")
    for packet in report["packets"]:
        readiness = packet["readiness"]
        lines.append(
            f"- {packet['packet_id']}: filing_status={packet['filing_status']}, "
            f"ledger_state={packet['ledger_state']}, "
            f"automation_gaps={readiness['automation_gap_count']}, "
            f"user_input_gaps={readiness['user_input_gap_count']}, "
            f"manual_review_items={readiness['manual_review_item_count']}"
        )
    return "\n".join(lines) + "\n"


def render_comparison_markdown(comparison: dict) -> str:
    lines = [
        "# Synthetic OCR Model Comparison",
        "",
        "## Summary",
    ]

    for model in comparison["models"]:
        if model.get("status") == "error":
            lines.append(
                f"- {model['model']}: error={model.get('error_summary') or model['error']}"
            )
            continue
        lines.append(
            f"- {model['model']}: documents={model['document_count']}, "
            f"exact={model['exact_match_count']}, "
            f"relaxed={model['relaxed_match_count']}, "
            f"content={model['content_match_count']}"
        )

    lines.append("")
    lines.append("## Filing Status Counts")
    for model in comparison["models"]:
        if model.get("status") == "error":
            lines.append(f"- {model['model']}: unavailable")
            continue
        counts = ", ".join(
            f"{key}={value}" for key, value in sorted(model["filing_status_counts"].items())
        )
        lines.append(f"- {model['model']}: {counts or 'none'}")

    lines.append("")
    lines.append("## Ledger State Counts")
    for model in comparison["models"]:
        if model.get("status") == "error":
            lines.append(f"- {model['model']}: unavailable")
            continue
        counts = ", ".join(
            f"{key}={value}" for key, value in sorted(model["ledger_state_counts"].items())
        )
        lines.append(f"- {model['model']}: {counts or 'none'}")

    return "\n".join(lines) + "\n"


def write_reports(output_dir: Path, model_reports: list[dict]) -> None:
    comparison = summarize_comparison(model_reports)
    write_json(output_dir / "ocr_comparison.json", comparison)
    (output_dir / "ocr_comparison.md").write_text(render_comparison_markdown(comparison))

    if len(model_reports) == 1:
        write_json(output_dir / "ocr_evaluation.json", model_reports[0])
        (output_dir / "ocr_evaluation.md").write_text(
            render_model_report_markdown(model_reports[0])
        )

    for report in model_reports:
        model_key = report["summary"]["model_key"]
        write_json(output_dir / f"ocr_evaluation_{model_key}.json", report)
        (output_dir / f"ocr_evaluation_{model_key}.md").write_text(
            render_model_report_markdown(report)
        )


def main() -> int:
    args = parse_args()
    models = resolve_models(args)
    output_dir = Path(args.output_dir)
    source_dir = output_dir / "source_corpus"
    rendered_dir = output_dir / "rendered"
    ingestion_dir = output_dir / "ingestion"
    output_dir.mkdir(parents=True, exist_ok=True)

    manifest = prepare_source_corpus(source_dir, args.packets)
    rendered_paths_by_packet = render_corpus_documents(manifest, source_dir, rendered_dir)
    model_reports = []
    for model in models:
        try:
            model_reports.append(
                evaluate_model(
                    manifest,
                    source_dir,
                    ingestion_dir,
                    rendered_paths_by_packet,
                    args,
                    model,
                )
            )
        except CommandError as err:
            model_reports.append(failed_model_report(model, args.location, str(err)))

    write_reports(output_dir, model_reports)
    print(render_comparison_markdown(summarize_comparison(model_reports)), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
