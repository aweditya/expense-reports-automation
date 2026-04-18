#!/usr/bin/env python3

import argparse
import json
import os
import subprocess
from collections import Counter
from pathlib import Path


DEFAULT_LOCATION = "global"
DEFAULT_PACKETS = 8
DEFAULT_MODEL = "gemini-3-flash-preview"


class CommandError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Stress test the managed ingestion workspace over a synthetic corpus, "
            "including OCR fidelity checks when rendered inputs are used."
        )
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where generated corpus, workspace bundles, and reports should be written",
    )
    parser.add_argument(
        "--workspace-root",
        help="Optional explicit workspace root; defaults to <output-dir>/workspace",
    )
    parser.add_argument(
        "--packets",
        type=int,
        default=DEFAULT_PACKETS,
        help="Number of synthetic packets to evaluate",
    )
    parser.add_argument(
        "--engine",
        choices=("builtin", "vertex-gemini-sdk"),
        default="builtin",
        help="Workspace transcription engine to evaluate",
    )
    parser.add_argument(
        "--fx",
        choices=("none", "demo"),
        default="demo",
        help="FX mode to pass through the workspace pipeline",
    )
    parser.add_argument("--project", help="Vertex project id")
    parser.add_argument(
        "--location",
        default=DEFAULT_LOCATION,
        help="Vertex location; defaults to global",
    )
    parser.add_argument(
        "--model",
        default=DEFAULT_MODEL,
        help="Gemini model id for live OCR runs",
    )
    parser.add_argument(
        "--service-account-key",
        help="Google Cloud service-account JSON key for live Gemini OCR",
    )
    parser.add_argument(
        "--sdk-python",
        help="Python interpreter for the Gemini SDK helper path",
    )
    parser.add_argument(
        "--render-inputs",
        action="store_true",
        help="Render markdown into OCR-style PNG/PDF inputs before evaluation",
    )
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def should_render_inputs(args: argparse.Namespace) -> bool:
    return args.render_inputs or args.engine != "builtin"


def default_sdk_python() -> str:
    env_value = os.environ.get("VERTEX_GEMINI_SDK_PYTHON")
    if env_value:
        return env_value
    venv_python = repo_root() / ".venv" / "bin" / "python"
    if venv_python.exists():
        return str(venv_python)
    return "python3"


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


def resolve_packet_input_paths(
    packet: dict,
    packet_source_dir: Path,
    packet_rendered_dir: Path,
    render_inputs: bool,
) -> dict[str, Path]:
    input_paths = {}
    for document in packet["documents"]:
        source_path = packet_source_dir / document["filename"]
        input_paths[document["filename"]] = (
            render_document(source_path, packet_rendered_dir, document["kind"])
            if render_inputs
            else source_path
        )
    return input_paths


def run_workspace_packet(
    packet_id: str,
    input_paths: list[Path],
    args: argparse.Namespace,
    workspace_root: Path,
) -> None:
    command = [
        "cargo",
        "run",
        "--bin",
        "ingest_bundle_workspace",
        "--",
        "stage-and-run",
        "--workspace-root",
        str(workspace_root),
        "--bundle-id",
        packet_id,
        "--run-id",
        sanitize_identifier(args.engine),
        "--fx",
        args.fx,
        "--engine",
        args.engine,
    ]
    if args.engine == "vertex-gemini-sdk":
        if args.project:
            command.extend(["--project", args.project])
        command.extend(["--location", args.location, "--model", args.model])
        if args.service_account_key:
            command.extend(["--service-account-key", args.service_account_key])
        if args.sdk_python:
            command.extend(["--sdk-python", args.sdk_python])
        elif default_sdk_python():
            command.extend(["--sdk-python", default_sdk_python()])
    command.extend(str(path) for path in input_paths)
    run_command(command, cwd=repo_root())


def packet_report(
    packet: dict,
    source_dir: Path,
    workspace_root: Path,
    render_inputs: bool,
) -> dict:
    bundle_id = packet["packet_id"]
    bundle_root = workspace_root / "bundles" / bundle_id
    workspace_manifest = read_json(bundle_root / "bundle_manifest.json")
    run = workspace_manifest["runs"][-1]
    run_artifacts_dir = bundle_root / run["output_dir"]
    readiness = summarize_readiness(read_json(run_artifacts_dir / "readiness.json"))

    document_results = []
    exact_match_count = 0
    relaxed_match_count = 0
    content_match_count = 0

    for document in packet["documents"]:
        source_path = source_dir / bundle_id / document["filename"]
        transcription_path = (
            run_artifacts_dir
            / "transcriptions"
            / f"{Path(document['filename']).stem}.transcribed.json"
        )
        comparison = compare_markdown(source_path, transcription_path)
        exact_match_count += int(comparison["exact_match"])
        relaxed_match_count += int(comparison["relaxed_match"])
        content_match_count += int(comparison["content_match"])
        document_results.append(
            {
                "kind": document["kind"],
                "source_markdown": str(source_path),
                "transcription_json": str(transcription_path),
                **comparison,
            }
        )

    return {
        "packet_id": bundle_id,
        "rendered_inputs": render_inputs,
        "current_stage": workspace_manifest["current_stage"],
        "filing_status": run["filing_status"],
        "ledger_state": run["ledger_state"],
        "workspace_bundle_manifest": str(bundle_root / "bundle_manifest.json"),
        "run_artifacts_dir": str(run_artifacts_dir),
        "readiness": readiness,
        "documents": document_results,
        "exact_match_count": exact_match_count,
        "relaxed_match_count": relaxed_match_count,
        "content_match_count": content_match_count,
    }


def evaluate_workspace_pipeline(
    manifest: dict,
    source_dir: Path,
    rendered_dir: Path,
    workspace_root: Path,
    args: argparse.Namespace,
) -> dict:
    render_inputs = should_render_inputs(args)
    packet_results = []
    failures = []
    filing_status_counts = Counter()
    ledger_state_counts = Counter()
    stage_counts = Counter()
    exact_match_count = 0
    relaxed_match_count = 0
    content_match_count = 0
    document_count = 0

    for packet in manifest["packets"]:
        packet_id = packet["packet_id"]
        packet_source_dir = source_dir / packet_id
        packet_rendered_dir = rendered_dir / packet_id
        packet_input_paths = resolve_packet_input_paths(
            packet,
            packet_source_dir,
            packet_rendered_dir,
            render_inputs,
        )
        try:
            run_workspace_packet(
                packet_id,
                list(packet_input_paths.values()),
                args,
                workspace_root,
            )
            report = packet_report(packet, source_dir, workspace_root, render_inputs)
        except CommandError as err:
            failures.append({"packet_id": packet_id, "error": str(err)})
            continue

        packet_results.append(report)
        filing_status_counts[report["filing_status"]] += 1
        ledger_state_counts[report["ledger_state"]] += 1
        stage_counts[report["current_stage"]] += 1
        exact_match_count += report["exact_match_count"]
        relaxed_match_count += report["relaxed_match_count"]
        content_match_count += report["content_match_count"]
        document_count += len(report["documents"])

    return {
        "summary": {
            "engine": args.engine,
            "rendered_inputs": render_inputs,
            "packet_count": len(manifest["packets"]),
            "successful_packet_count": len(packet_results),
            "failure_count": len(failures),
            "document_count": document_count,
            "exact_match_count": exact_match_count,
            "relaxed_match_count": relaxed_match_count,
            "content_match_count": content_match_count,
            "filing_status_counts": dict(filing_status_counts),
            "ledger_state_counts": dict(ledger_state_counts),
            "stage_counts": dict(stage_counts),
        },
        "failures": failures,
        "packets": packet_results,
    }


def render_report_markdown(report: dict) -> str:
    summary = report["summary"]
    lines = [
        "# Workspace Pipeline Evaluation",
        "",
        f"- engine: {summary['engine']}",
        f"- rendered_inputs: {str(summary['rendered_inputs']).lower()}",
        f"- packets: {summary['packet_count']}",
        f"- successful_packets: {summary['successful_packet_count']}",
        f"- failures: {summary['failure_count']}",
        f"- documents: {summary['document_count']}",
        f"- exact markdown matches: {summary['exact_match_count']}",
        f"- relaxed markdown matches: {summary['relaxed_match_count']}",
        f"- content-only matches: {summary['content_match_count']}",
        "",
        "## Workspace Stages",
    ]

    for stage, count in sorted(summary["stage_counts"].items()):
        lines.append(f"- {stage}: {count}")

    lines.extend(["", "## Filing Status Counts"])
    for status, count in sorted(summary["filing_status_counts"].items()):
        lines.append(f"- {status}: {count}")

    lines.extend(["", "## Ledger State Counts"])
    for state, count in sorted(summary["ledger_state_counts"].items()):
        lines.append(f"- {state}: {count}")

    if report["failures"]:
        lines.extend(["", "## Failures"])
        for failure in report["failures"]:
            lines.append(f"- {failure['packet_id']}: {failure['error'].splitlines()[-1]}")

    lines.extend(["", "## Packet Results"])
    for packet in report["packets"]:
        lines.append(
            "- {packet_id}: stage={stage}, filing_status={filing_status}, ledger_state={ledger_state}, "
            "automation_gaps={automation_gap_count}, user_input_gaps={user_input_gap_count}, "
            "manual_review_items={manual_review_item_count}, exact={exact}, relaxed={relaxed}, content={content}".format(
                packet_id=packet["packet_id"],
                stage=packet["current_stage"],
                filing_status=packet["filing_status"],
                ledger_state=packet["ledger_state"],
                automation_gap_count=packet["readiness"]["automation_gap_count"],
                user_input_gap_count=packet["readiness"]["user_input_gap_count"],
                manual_review_item_count=packet["readiness"]["manual_review_item_count"],
                exact=packet["exact_match_count"],
                relaxed=packet["relaxed_match_count"],
                content=packet["content_match_count"],
            )
        )

    return "\n".join(lines)


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
    return "".join(cleaned).strip("_") or "value"


def main() -> int:
    args = parse_args()
    output_dir = Path(args.output_dir)
    source_dir = output_dir / "source_corpus"
    rendered_dir = output_dir / "rendered"
    workspace_root = Path(args.workspace_root) if args.workspace_root else output_dir / "workspace"
    source_dir.mkdir(parents=True, exist_ok=True)
    rendered_dir.mkdir(parents=True, exist_ok=True)
    workspace_root.mkdir(parents=True, exist_ok=True)

    manifest = prepare_source_corpus(source_dir, args.packets)
    report = evaluate_workspace_pipeline(
        manifest,
        source_dir,
        rendered_dir,
        workspace_root,
        args,
    )

    report_json_path = output_dir / "workspace_evaluation.json"
    report_markdown_path = output_dir / "workspace_evaluation.md"
    write_json(report_json_path, report)
    report_markdown_path.write_text(render_report_markdown(report))
    print(report_markdown_path.read_text())
    return 0 if not report["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
