#!/usr/bin/env python3

import argparse
import json
import os
import subprocess
from collections import Counter
from pathlib import Path


DEFAULT_MODEL = "gemini-3-flash-preview"
DEFAULT_LOCATION = "global"
DEFAULT_PACKETS = 2


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run end-to-end OCR ingestion on a rendered synthetic corpus and score "
            "markdown fidelity against the source markdown."
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
        default=DEFAULT_MODEL,
        help="Gemini 3 model id to use for OCR",
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


def run_command(command: list[str], cwd: Path) -> None:
    subprocess.run(command, cwd=cwd, check=True)


def read_json(path: Path) -> dict:
    return json.loads(path.read_text())


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


def ingest_packet(
    packet_id: str,
    rendered_paths: list[Path],
    output_dir: Path,
    args: argparse.Namespace,
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
        args.model,
        "--sdk-python",
        args.sdk_python or default_sdk_python(),
    ]
    if args.project:
        command.extend(["--project", args.project])
    command.extend(str(path) for path in rendered_paths)
    run_command(command, cwd=repo_root())


def render_report_markdown(report: dict) -> str:
    lines = [
        "# Synthetic OCR Evaluation",
        "",
        f"- packets: {report['summary']['packet_count']}",
        f"- documents: {report['summary']['document_count']}",
        f"- model: {report['summary']['model']}",
        f"- exact markdown matches: {report['summary']['exact_match_count']}",
        f"- relaxed markdown matches: {report['summary']['relaxed_match_count']}",
        f"- content-only matches: {report['summary']['content_match_count']}",
        "",
        "## Filing Status Counts",
    ]
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
        lines.extend(
            [
                f"- {packet['packet_id']}: filing_status={packet['filing_status']}, "
                f"ledger_state={packet['ledger_state']}, "
                f"automation_gaps={readiness['automation_gap_count']}, "
                f"user_input_gaps={readiness['user_input_gap_count']}, "
                f"manual_review_items={readiness['manual_review_item_count']}",
            ]
        )
    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    output_dir = Path(args.output_dir)
    source_dir = output_dir / "source_corpus"
    rendered_dir = output_dir / "rendered"
    ingestion_dir = output_dir / "ingestion"
    output_dir.mkdir(parents=True, exist_ok=True)

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
            str(args.packets),
        ],
        cwd=repo_root(),
    )

    manifest = read_json(source_dir / "manifest.json")
    packet_results = []
    exact_match_count = 0
    relaxed_match_count = 0
    content_match_count = 0
    filing_status_counts: Counter[str] = Counter()
    ledger_state_counts: Counter[str] = Counter()
    document_count = 0

    for packet in manifest["packets"]:
        packet_id = packet["packet_id"]
        packet_source_dir = source_dir / packet_id
        packet_rendered_dir = rendered_dir / packet_id
        packet_ingestion_dir = ingestion_dir / packet_id
        packet_ingestion_dir.mkdir(parents=True, exist_ok=True)

        rendered_paths = []
        for document in packet["documents"]:
            source_path = packet_source_dir / document["filename"]
            rendered_paths.append(
                render_document(source_path, packet_rendered_dir, document["kind"])
            )

        ingest_packet(packet_id, rendered_paths, packet_ingestion_dir, args)

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
                path for path in rendered_paths if path.stem == source_path.stem
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
                "readiness": {
                    "automation_gap_count": readiness["automation_gap_count"],
                    "user_input_gap_count": readiness["user_input_gap_count"],
                    "manual_review_item_count": readiness["manual_review_item_count"],
                    "other_warning_count": readiness["other_warning_count"],
                },
                "documents": packet_document_results,
                "draft_version_count": ledger["summary"]["draft_version_count"],
                "submission_attempt_count": ledger["summary"]["submission_attempt_count"],
            }
        )

    report = {
        "summary": {
            "packet_count": len(packet_results),
            "document_count": document_count,
            "model": args.model,
            "location": args.location,
            "exact_match_count": exact_match_count,
            "relaxed_match_count": relaxed_match_count,
            "content_match_count": content_match_count,
            "filing_status_counts": dict(filing_status_counts),
            "ledger_state_counts": dict(ledger_state_counts),
        },
        "packets": packet_results,
    }
    (output_dir / "ocr_evaluation.json").write_text(json.dumps(report, indent=2))
    (output_dir / "ocr_evaluation.md").write_text(render_report_markdown(report))

    print(render_report_markdown(report), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
