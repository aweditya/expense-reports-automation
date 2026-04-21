#!/usr/bin/env python3

import argparse
import json
import os
import re
import subprocess
from collections import Counter
from pathlib import Path


DEFAULT_MODELS = ["gemini-3-flash-preview", "gemini-3-pro-preview"]
DEFAULT_LOCATION = "global"
DEFAULT_PACKETS = 4
DEFAULT_COMPARE_PROFILES = (
    ("table_focused_binarized", "table_focused", "binarized"),
    ("verification_contrast_boosted", "verification", "contrast_boosted"),
)
SUPPORTED_PASS_KINDS = {"primary", "verification", "table_focused", "geometry_assist"}
SUPPORTED_PREPROCESS_VARIANTS = {
    "original",
    "contrast_boosted",
    "grayscale",
    "binarized",
    "deskewed",
}
GROUNDABLE_RECEIPT_FIELDS = (
    "merchant_name",
    "transaction_date",
    "total_paid",
    "total_paid_currency",
)


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
        "--corpus-manifest",
        help=(
            "Optional path to a fixed OCR corpus manifest. When set, the evaluator "
            "uses those input documents instead of generating a synthetic corpus."
        ),
    )
    parser.add_argument(
        "--sdk-python",
        help="Python interpreter to use for the Gemini SDK helper path",
    )
    parser.add_argument(
        "--compare-passes",
        action="store_true",
        help=(
            "For receipt documents, run an additional table-focused OCR pass and "
            "compare it against the primary ingestion transcription."
        ),
    )
    parser.add_argument(
        "--compare-profile",
        action="append",
        dest="compare_profiles",
        help=(
            "Additional OCR comparison profile in name:pass_kind:preprocess form. "
            "Repeat to benchmark multiple receipt OCR lanes. Defaults to "
            "table_focused_binarized:table_focused:binarized and "
            "verification_contrast_boosted:verification:contrast_boosted."
        ),
    )
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def default_sdk_python() -> str:
    env_value = os.environ.get("VERTEX_GEMINI_SDK_PYTHON")
    if env_value:
        return env_value
    venv_python = repo_root() / ".venv" / "bin" / "python"
    if venv_python.exists():
        return str(venv_python)
    return "python3"


def resolve_models(args: argparse.Namespace) -> list[str]:
    return args.models or list(DEFAULT_MODELS)


def resolve_compare_profiles(args: argparse.Namespace) -> list[dict]:
    raw_profiles = args.compare_profiles or [
        ":".join(profile) for profile in DEFAULT_COMPARE_PROFILES
    ]
    profiles = []
    for raw in raw_profiles:
        parts = raw.split(":")
        if len(parts) != 3:
            raise ValueError(
                f"invalid compare profile {raw!r}; expected name:pass_kind:preprocess_variant"
            )
        name, pass_kind, preprocess_variant = parts
        if not name:
            raise ValueError("compare profile name must not be empty")
        if pass_kind not in SUPPORTED_PASS_KINDS:
            raise ValueError(
                f"unsupported compare profile pass kind {pass_kind!r}; "
                f"expected one of {', '.join(sorted(SUPPORTED_PASS_KINDS))}"
            )
        if preprocess_variant not in SUPPORTED_PREPROCESS_VARIANTS:
            raise ValueError(
                f"unsupported compare profile preprocess variant {preprocess_variant!r}; "
                f"expected one of {', '.join(sorted(SUPPORTED_PREPROCESS_VARIANTS))}"
            )
        profiles.append(
            {
                "name": sanitize_identifier(name),
                "pass_kind": pass_kind,
                "preprocess_variant": preprocess_variant,
            }
        )
    return profiles


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


def normalize_field_value(value):
    if value is None:
        return None
    if isinstance(value, (int, float)):
        return str(value)
    return " ".join(str(value).strip().split()).lower()


def normalize_merchant_name(value):
    normalized = normalize_field_value(value)
    if normalized is None:
        return None
    tokens = normalized.replace(".", " ").replace(",", " ").split()
    if tokens and tokens[-1].startswith("(") and any(ch.isdigit() for ch in tokens[-1]):
        tokens = tokens[:-1]
    normalized = " ".join(tokens)
    normalized = (
        normalized.replace("(", " ")
        .replace(")", " ")
        .replace("&", " and ")
    )
    return " ".join(normalized.split())


def normalize_date_value(value):
    normalized = normalize_field_value(value)
    if normalized is None:
        return None
    for token in normalized.split():
        token = token.strip(",;()")
        if looks_like_date_token(token):
            return token
    return normalized


def looks_like_date_token(value):
    for separator in ("/", "-"):
        parts = value.split(separator)
        if len(parts) == 3 and all(part.isdigit() for part in parts):
            return True
    return False


def facts_value_for_expected_field(facts_payload: dict, field_name: str):
    if field_name == "classification_kind":
        return facts_payload.get("classification", {}).get("kind")
    if field_name == "line_item_count":
        receipt = facts_payload.get("facts", {}).get("receipt") or {}
        return len(receipt.get("line_items") or [])

    receipt = facts_payload.get("facts", {}).get("receipt") or {}

    if field_name == "merchant_name":
        return (((receipt.get("merchant_name") or {}).get("value")))
    if field_name == "transaction_date":
        return (((receipt.get("transaction_date") or {}).get("value")))
    if field_name == "total_paid":
        return ((((receipt.get("total_paid") or {}).get("value") or {}).get("amount")))
    if field_name == "total_paid_currency":
        return ((((receipt.get("total_paid") or {}).get("value") or {}).get("currency")))
    if field_name == "subtotal":
        return ((((receipt.get("subtotal") or {}).get("value") or {}).get("amount")))
    if field_name == "tax_amount":
        return ((((receipt.get("tax_amount") or {}).get("value") or {}).get("amount")))
    if field_name == "tip_amount":
        return ((((receipt.get("tip_amount") or {}).get("value") or {}).get("amount")))
    return None


def compare_expected_fields(expected_fields: dict, facts_path: Path) -> dict:
    facts_payload = read_json(facts_path)
    field_results = []
    match_count = 0

    for field_name, expected_value in expected_fields.items():
        actual_value = facts_value_for_expected_field(facts_payload, field_name)
        if field_name == "merchant_name":
            matched = normalize_merchant_name(expected_value) == normalize_merchant_name(
                actual_value
            )
        elif field_name == "transaction_date":
            matched = normalize_date_value(expected_value) == normalize_date_value(actual_value)
        else:
            matched = normalize_field_value(expected_value) == normalize_field_value(actual_value)
        match_count += int(matched)
        field_results.append(
            {
                "field": field_name,
                "expected": expected_value,
                "actual": actual_value,
                "matched": matched,
            }
        )

    return {
        "expected_field_count": len(field_results),
        "matched_field_count": match_count,
        "fields": field_results,
    }


def join_region_texts_by_id(transcription_payload: dict) -> dict[str, list[str]]:
    region_texts = {}
    for page in transcription_payload.get("pages") or []:
        for region in page.get("regions") or []:
            region_id = str(region.get("region_id") or "").strip()
            text = str(region.get("text") or "").strip()
            if not region_id or not text:
                continue
            region_texts.setdefault(region_id, []).append(text)
    return region_texts


def all_region_texts(transcription_payload: dict) -> list[str]:
    texts = []
    for page in transcription_payload.get("pages") or []:
        for region in page.get("regions") or []:
            text = str(region.get("text") or "").strip()
            if text:
                texts.append(text)
    return texts


def first_numeric_token(value):
    if value is None:
        return None
    matches = re.findall(r"\d+(?:[.,]\d+)?", str(value))
    if not matches:
        return None
    return matches[0].replace(",", ".")


def normalize_currency_token(value):
    normalized = normalize_field_value(value)
    if normalized is None:
        return None
    tokens = re.findall(r"[a-z$]+", normalized)
    for token in tokens:
        if token == "rm":
            return "myr"
        if token in {"myr", "sgd", "usd"}:
            return token
        if token == "sg":
            continue
    if "sg$" in normalized:
        return "sgd"
    if "us$" in normalized:
        return "usd"
    return normalized


def grounding_field_matches(field_name: str, expected_value, actual_text: str | None) -> bool:
    if actual_text is None:
        return False
    if field_name == "merchant_name":
        expected = normalize_merchant_name(expected_value)
        actual = normalize_merchant_name(actual_text)
        return expected is not None and actual is not None and expected == actual
    if field_name == "transaction_date":
        expected = normalize_date_value(expected_value)
        actual = normalize_date_value(actual_text)
        return expected is not None and actual is not None and expected == actual
    if field_name == "total_paid":
        expected = first_numeric_token(expected_value)
        actual = first_numeric_token(actual_text)
        return expected is not None and actual is not None and expected == actual
    if field_name == "total_paid_currency":
        expected = normalize_currency_token(expected_value)
        actual = normalize_currency_token(actual_text)
        if expected is None or actual is None:
            return False
        return expected == actual or expected in normalize_field_value(actual_text)
    return False


def compare_expected_grounding(expected_fields: dict, transcription_path: Path) -> dict | None:
    grounded_expected_fields = {
        field_name: expected_value
        for field_name, expected_value in expected_fields.items()
        if field_name in GROUNDABLE_RECEIPT_FIELDS
    }
    if not grounded_expected_fields:
        return None

    transcription_payload = read_json(transcription_path)
    region_texts = join_region_texts_by_id(transcription_payload)
    fallback_candidates = all_region_texts(transcription_payload)
    field_results = []
    match_count = 0

    for field_name, expected_value in grounded_expected_fields.items():
        candidates = region_texts.get(field_name) or fallback_candidates
        matched_text = next(
            (
                candidate
                for candidate in candidates
                if grounding_field_matches(field_name, expected_value, candidate)
            ),
            None,
        )
        matched = matched_text is not None
        match_count += int(matched)
        field_results.append(
            {
                "field": field_name,
                "expected": expected_value,
                "region_count": len(candidates),
                "actual_candidates": candidates,
                "matched_candidate": matched_text,
                "matched": matched,
            }
        )

    return {
        "geometry_available": bool(transcription_payload.get("metadata", {}).get("geometry_available")),
        "geometry_source": transcription_payload.get("metadata", {}).get("geometry_source") or "none",
        "expected_field_count": len(field_results),
        "matched_field_count": match_count,
        "fields": field_results,
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


def render_corpus_documents(
    manifest: dict, source_dir: Path, rendered_dir: Path
) -> dict[str, list[Path]]:
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


def resolve_manifest_path(manifest_root: Path, value: str) -> Path:
    path = Path(value)
    if path.is_absolute():
        return path

    direct = (manifest_root / path).resolve()
    if direct.exists():
        return direct

    corpus_root_candidate = manifest_root.parent
    fallback = (corpus_root_candidate / path).resolve()
    return fallback


def build_synthetic_corpus_spec(
    manifest: dict,
    source_dir: Path,
    rendered_paths_by_packet: dict[str, list[Path]],
) -> dict:
    packets = []
    for packet in manifest["packets"]:
        packet_id = packet["packet_id"]
        packet_source_dir = source_dir / packet_id
        documents = []
        for document in packet["documents"]:
            source_path = packet_source_dir / document["filename"]
            input_path = next(
                path
                for path in rendered_paths_by_packet[packet_id]
                if path.stem == source_path.stem
            )
            documents.append(
                {
                    "document_id": source_path.stem,
                    "kind": document["kind"],
                    "input_path": str(input_path),
                    "ground_truth_markdown_path": str(source_path),
                    "expected_fields": document.get("expected_fields") or {},
                    "transcription_stem": source_path.stem,
                }
            )
        packets.append({"packet_id": packet_id, "documents": documents})

    return {
        "corpus_name": manifest.get("corpus_name", "synthetic_receipt_corpus"),
        "packets": packets,
    }


def build_manifest_corpus_spec(manifest_path: Path) -> dict:
    manifest_root = manifest_path.parent.resolve()
    manifest = read_json(manifest_path)
    packets = []

    if "packets" in manifest:
        packet_entries = manifest["packets"]
    elif "documents" in manifest:
        packet_entries = [
            {"packet_id": document.get("packet_id") or document["document_id"], "documents": [document]}
            for document in manifest["documents"]
        ]
    else:
        raise SystemExit("corpus manifest must contain either top-level packets or documents")

    for packet in packet_entries:
        packet_id = packet["packet_id"]
        documents = []
        for document in packet["documents"]:
            input_path = resolve_manifest_path(manifest_root, document["input_path"])
            ground_truth_markdown_path = document.get("ground_truth_markdown_path")
            transcription_stem = document.get("transcription_stem") or input_path.stem
            documents.append(
                {
                    "document_id": document.get("document_id") or transcription_stem,
                    "kind": document.get("kind", "receipt"),
                    "input_path": str(input_path),
                    "ground_truth_markdown_path": str(
                        resolve_manifest_path(manifest_root, ground_truth_markdown_path)
                    )
                    if ground_truth_markdown_path
                    else None,
                    "expected_fields": document.get("expected_fields") or {},
                    "transcription_stem": transcription_stem,
                }
            )
        packets.append({"packet_id": packet_id, "documents": documents})

    return {
        "corpus_name": manifest.get("corpus_name", manifest_path.stem),
        "packets": packets,
    }


def ingest_packet(
    packet_id: str,
    input_paths: list[Path],
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
    command.extend(str(path) for path in input_paths)
    run_command(command, cwd=repo_root())


def compare_receipt_passes(
    packet_id: str,
    document: dict,
    primary_transcription_path: Path,
    packet_ingestion_dir: Path,
    args: argparse.Namespace,
    model: str,
) -> dict:
    compare_dir = (
        packet_ingestion_dir
        / "ocr_pass_comparisons"
        / document["transcription_stem"]
    )
    compare_dir.mkdir(parents=True, exist_ok=True)
    profiles = []
    for profile in resolve_compare_profiles(args):
        profile_dir = compare_dir / profile["name"]
        profile_dir.mkdir(parents=True, exist_ok=True)
        secondary_transcription_path = (
            profile_dir / f"{document['transcription_stem']}.{profile['name']}.json"
        )
        comparison_json_path = profile_dir / "comparison.json"
        comparison_md_path = profile_dir / "comparison.md"
        comparison_html_path = profile_dir / "comparison.html"

        transcribe_command = [
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
            args.location,
            "--model",
            model,
            "--sdk-python",
            args.sdk_python or default_sdk_python(),
            "--format",
            "json",
            "--output",
            str(secondary_transcription_path),
            "--pass-kind",
            profile["pass_kind"],
            "--preprocess-variant",
            profile["preprocess_variant"],
            "--pass-id",
            f"{packet_id}_{document['transcription_stem']}_{profile['name']}",
        ]
        if args.project:
            transcribe_command.extend(["--project", args.project])
        transcribe_command.append(document["input_path"])
        run_command(transcribe_command, cwd=repo_root())

        for output_format, output_path in [
            ("json", comparison_json_path),
            ("markdown", comparison_md_path),
            ("html", comparison_html_path),
        ]:
            compare_command = [
                "cargo",
                "run",
                "--bin",
                "compare_ocr_passes",
                "--",
                "--format",
                output_format,
                "--output",
                str(output_path),
                str(primary_transcription_path),
                str(secondary_transcription_path),
            ]
            run_command(compare_command, cwd=repo_root())

        comparison = read_json(comparison_json_path)
        profiles.append(
            {
                "name": profile["name"],
                "pass_kind": profile["pass_kind"],
                "preprocess_variant": profile["preprocess_variant"],
                "secondary_transcription_path": str(secondary_transcription_path),
                "json_path": str(comparison_json_path),
                "markdown_path": str(comparison_md_path),
                "html_path": str(comparison_html_path),
                "comparison": comparison,
                "summary": summarize_pass_comparison(comparison),
            }
        )

    return {
        "profiles": profiles,
        "profile_count": len(profiles),
    }


def empty_pass_profile_summary(name: str) -> dict:
    return {
        "profile_name": name,
        "run_count": 0,
        "divergent_run_count": 0,
        "disagreement_count": 0,
        "confidence_counts": {},
        "field_confidence_counts": {},
        "field_status_counts": {},
    }


def summarize_pass_comparison(comparison: dict) -> dict:
    confidence = comparison.get("overall_confidence") or "unknown"
    disagreement_count = comparison.get("disagreement_count", 0)
    field_confidence_counts: Counter[str] = Counter()
    field_status_counts: Counter[str] = Counter()
    divergent_fields = []
    for field in comparison.get("fields") or []:
        field_confidence = field.get("confidence") or "unknown"
        field_status = field.get("status") or "unknown"
        field_confidence_counts[field_confidence] += 1
        field_status_counts[field_status] += 1
        if field_status == "divergent":
            divergent_fields.append(field.get("field"))
    return {
        "overall_confidence": confidence,
        "disagreement_count": disagreement_count,
        "has_divergence": disagreement_count > 0,
        "field_confidence_counts": dict(field_confidence_counts),
        "field_status_counts": dict(field_status_counts),
        "divergent_fields": [field for field in divergent_fields if field],
    }


def evaluate_model(
    corpus_spec: dict,
    ingestion_root: Path,
    args: argparse.Namespace,
    model: str,
) -> dict:
    packet_results = []
    exact_match_count = 0
    relaxed_match_count = 0
    content_match_count = 0
    comparable_document_count = 0
    expected_field_count = 0
    matched_field_count = 0
    grounding_document_count = 0
    grounding_available_document_count = 0
    grounding_fully_matched_document_count = 0
    grounding_expected_field_count = 0
    grounding_matched_field_count = 0
    grounding_field_match_counts: Counter[str] = Counter()
    pass_comparison_document_count = 0
    pass_comparison_profile_run_count = 0
    pass_comparison_divergent_document_count = 0
    pass_comparison_disagreement_count = 0
    pass_comparison_confidence_counts: Counter[str] = Counter()
    pass_comparison_field_confidence_counts: Counter[str] = Counter()
    pass_comparison_field_status_counts: Counter[str] = Counter()
    pass_comparison_profile_summaries: dict[str, dict] = {}
    filing_status_counts: Counter[str] = Counter()
    ledger_state_counts: Counter[str] = Counter()
    document_count = 0
    inspection_document_count = 0
    model_key = sanitize_identifier(model)

    for packet in corpus_spec["packets"]:
        packet_id = packet["packet_id"]
        packet_ingestion_dir = ingestion_root / model_key / packet_id
        packet_ingestion_dir.mkdir(parents=True, exist_ok=True)

        ingest_packet(
            packet_id,
            [Path(document["input_path"]) for document in packet["documents"]],
            packet_ingestion_dir,
            args,
            model,
        )

        readiness = summarize_readiness(read_json(packet_ingestion_dir / "readiness.json"))
        manifest_entry = read_json(packet_ingestion_dir / "manifest.json")
        ledger = read_json(packet_ingestion_dir / "ledger.json")

        packet_document_results = []
        for document in packet["documents"]:
            transcription_path = (
                packet_ingestion_dir
                / "transcriptions"
                / f"{document['transcription_stem']}.transcribed.json"
            )
            facts_path = (
                packet_ingestion_dir / "facts" / f"{document['transcription_stem']}.facts.json"
            )
            ground_truth_markdown_path = document.get("ground_truth_markdown_path")
            comparison = None
            if ground_truth_markdown_path:
                comparison = compare_markdown(
                    Path(ground_truth_markdown_path),
                    transcription_path,
                )
                exact_match_count += int(comparison["exact_match"])
                relaxed_match_count += int(comparison["relaxed_match"])
                content_match_count += int(comparison["content_match"])
                comparable_document_count += 1
            expected_fields = document.get("expected_fields") or {}
            field_comparison = None
            if expected_fields:
                field_comparison = compare_expected_fields(expected_fields, facts_path)
                expected_field_count += field_comparison["expected_field_count"]
                matched_field_count += field_comparison["matched_field_count"]
            grounding_comparison = compare_expected_grounding(
                expected_fields,
                transcription_path,
            )
            if grounding_comparison:
                grounding_document_count += 1
                grounding_available_document_count += int(
                    grounding_comparison["geometry_available"]
                )
                grounding_expected_field_count += grounding_comparison["expected_field_count"]
                grounding_matched_field_count += grounding_comparison["matched_field_count"]
                grounding_fully_matched_document_count += int(
                    grounding_comparison["expected_field_count"] > 0
                    and grounding_comparison["expected_field_count"]
                    == grounding_comparison["matched_field_count"]
                )
                for field in grounding_comparison["fields"]:
                    grounding_field_match_counts[field["field"]] += int(field["matched"])
            pass_comparison = None
            comparison_html_path = None
            if args.compare_passes and document["kind"] == "receipt":
                pass_comparison = compare_receipt_passes(
                    packet_id,
                    document,
                    transcription_path,
                    packet_ingestion_dir,
                    args,
                    model,
                )
                pass_comparison_document_count += 1
                document_has_divergence = False
                for profile_run in pass_comparison["profiles"]:
                    pass_summary = profile_run["summary"]
                    pass_comparison_profile_run_count += 1
                    pass_comparison_disagreement_count += pass_summary["disagreement_count"]
                    document_has_divergence = (
                        document_has_divergence or pass_summary["has_divergence"]
                    )
                    pass_comparison_confidence_counts[pass_summary["overall_confidence"]] += 1
                    for key, value in pass_summary["field_confidence_counts"].items():
                        pass_comparison_field_confidence_counts[key] += value
                    for key, value in pass_summary["field_status_counts"].items():
                        pass_comparison_field_status_counts[key] += value

                    profile_summary = pass_comparison_profile_summaries.setdefault(
                        profile_run["name"],
                        empty_pass_profile_summary(profile_run["name"]),
                    )
                    profile_summary["run_count"] += 1
                    profile_summary["divergent_run_count"] += int(
                        pass_summary["has_divergence"]
                    )
                    profile_summary["disagreement_count"] += pass_summary[
                        "disagreement_count"
                    ]
                    confidence_counts = Counter(profile_summary["confidence_counts"])
                    field_confidence_counts = Counter(
                        profile_summary["field_confidence_counts"]
                    )
                    field_status_counts = Counter(profile_summary["field_status_counts"])
                    confidence_counts[pass_summary["overall_confidence"]] += 1
                    for key, value in pass_summary["field_confidence_counts"].items():
                        field_confidence_counts[key] += value
                    for key, value in pass_summary["field_status_counts"].items():
                        field_status_counts[key] += value
                    profile_summary["confidence_counts"] = dict(confidence_counts)
                    profile_summary["field_confidence_counts"] = dict(
                        field_confidence_counts
                    )
                    profile_summary["field_status_counts"] = dict(field_status_counts)

                pass_comparison_divergent_document_count += int(document_has_divergence)
                comparison_html_path = next(
                    (
                        profile_run["html_path"]
                        for profile_run in pass_comparison["profiles"]
                    ),
                    None,
                )
            document_count += 1
            inspection_path = (
                packet_ingestion_dir
                / "ocr_inspection"
                / document["transcription_stem"]
                / "inspection.html"
            )
            grounding_html_path = (
                packet_ingestion_dir
                / "ocr_grounding"
                / document["transcription_stem"]
                / "grounded_preview.html"
            )
            inspection_document_count += int(inspection_path.exists())

            packet_document_results.append(
                {
                    "kind": document["kind"],
                    "document_id": document["document_id"],
                    "source_markdown": ground_truth_markdown_path,
                    "input_document": document["input_path"],
                    "transcription_json": str(transcription_path),
                    "facts_json": str(facts_path),
                    "ocr_inspection_html": str(inspection_path) if inspection_path.exists() else None,
                    "ocr_grounding_html": str(grounding_html_path)
                    if grounding_html_path.exists()
                    else None,
                    "ocr_pass_comparison_html": comparison_html_path,
                    "ocr_pass_comparison_profiles": [
                        {
                            "name": profile_run["name"],
                            "html_path": profile_run["html_path"],
                            "summary": profile_run["summary"],
                        }
                        for profile_run in (pass_comparison or {}).get("profiles", [])
                    ],
                    "exact_match": comparison["exact_match"] if comparison else None,
                    "relaxed_match": comparison["relaxed_match"] if comparison else None,
                    "content_match": comparison["content_match"] if comparison else None,
                    "expected_fields": field_comparison,
                    "expected_grounding": grounding_comparison,
                    "ocr_pass_comparison": pass_comparison,
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
            "corpus_name": corpus_spec.get("corpus_name"),
            "packet_count": len(packet_results),
            "document_count": document_count,
            "comparable_document_count": comparable_document_count,
            "expected_field_count": expected_field_count,
            "matched_field_count": matched_field_count,
            "grounding_document_count": grounding_document_count,
            "grounding_available_document_count": grounding_available_document_count,
            "grounding_fully_matched_document_count": grounding_fully_matched_document_count,
            "grounding_expected_field_count": grounding_expected_field_count,
            "grounding_matched_field_count": grounding_matched_field_count,
            "grounding_field_match_counts": dict(grounding_field_match_counts),
            "pass_comparison_document_count": pass_comparison_document_count,
            "pass_comparison_profile_run_count": pass_comparison_profile_run_count,
            "pass_comparison_divergent_document_count": pass_comparison_divergent_document_count,
            "pass_comparison_disagreement_count": pass_comparison_disagreement_count,
            "pass_comparison_confidence_counts": dict(pass_comparison_confidence_counts),
            "pass_comparison_field_confidence_counts": dict(
                pass_comparison_field_confidence_counts
            ),
            "pass_comparison_field_status_counts": dict(pass_comparison_field_status_counts),
            "pass_comparison_profile_summaries": dict(
                sorted(pass_comparison_profile_summaries.items())
            ),
            "model": model,
            "model_key": model_key,
            "location": args.location,
            "exact_match_count": exact_match_count,
            "relaxed_match_count": relaxed_match_count,
            "content_match_count": content_match_count,
            "filing_status_counts": dict(filing_status_counts),
            "ledger_state_counts": dict(ledger_state_counts),
            "inspection_document_count": inspection_document_count,
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
                "expected_field_count": report["summary"].get("expected_field_count", 0),
                "matched_field_count": report["summary"].get("matched_field_count", 0),
                "grounding_document_count": report["summary"].get(
                    "grounding_document_count", 0
                ),
                "grounding_available_document_count": report["summary"].get(
                    "grounding_available_document_count", 0
                ),
                "grounding_fully_matched_document_count": report["summary"].get(
                    "grounding_fully_matched_document_count", 0
                ),
                "grounding_expected_field_count": report["summary"].get(
                    "grounding_expected_field_count", 0
                ),
                "grounding_matched_field_count": report["summary"].get(
                    "grounding_matched_field_count", 0
                ),
                "grounding_field_match_counts": report["summary"].get(
                    "grounding_field_match_counts", {}
                ),
                "pass_comparison_document_count": report["summary"].get(
                    "pass_comparison_document_count", 0
                ),
                "pass_comparison_profile_run_count": report["summary"].get(
                    "pass_comparison_profile_run_count", 0
                ),
                "pass_comparison_divergent_document_count": report["summary"].get(
                    "pass_comparison_divergent_document_count", 0
                ),
                "pass_comparison_disagreement_count": report["summary"].get(
                    "pass_comparison_disagreement_count", 0
                ),
                "pass_comparison_confidence_counts": report["summary"].get(
                    "pass_comparison_confidence_counts", {}
                ),
                "pass_comparison_field_confidence_counts": report["summary"].get(
                    "pass_comparison_field_confidence_counts", {}
                ),
                "pass_comparison_field_status_counts": report["summary"].get(
                    "pass_comparison_field_status_counts", {}
                ),
                "pass_comparison_profile_summaries": report["summary"].get(
                    "pass_comparison_profile_summaries", {}
                ),
                "exact_match_count": report["summary"]["exact_match_count"],
                "relaxed_match_count": report["summary"]["relaxed_match_count"],
                "content_match_count": report["summary"]["content_match_count"],
                "filing_status_counts": report["summary"]["filing_status_counts"],
                "ledger_state_counts": report["summary"]["ledger_state_counts"],
                "inspection_document_count": report["summary"].get(
                    "inspection_document_count", 0
                ),
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
            "comparable_document_count": 0,
            "expected_field_count": 0,
            "matched_field_count": 0,
            "grounding_document_count": 0,
            "grounding_available_document_count": 0,
            "grounding_fully_matched_document_count": 0,
            "grounding_expected_field_count": 0,
            "grounding_matched_field_count": 0,
            "grounding_field_match_counts": {},
            "pass_comparison_document_count": 0,
            "pass_comparison_profile_run_count": 0,
            "pass_comparison_divergent_document_count": 0,
            "pass_comparison_disagreement_count": 0,
            "pass_comparison_confidence_counts": {},
            "pass_comparison_field_confidence_counts": {},
            "pass_comparison_field_status_counts": {},
            "pass_comparison_profile_summaries": {},
            "exact_match_count": 0,
            "relaxed_match_count": 0,
            "content_match_count": 0,
            "filing_status_counts": {},
            "ledger_state_counts": {},
            "inspection_document_count": 0,
        },
        "packets": [],
    }


def render_model_report_markdown(report: dict) -> str:
    lines = [
        "# OCR Evaluation",
        "",
        f"- status: {report['summary'].get('status', 'ok')}",
        f"- corpus: {report['summary'].get('corpus_name') or 'unnamed'}",
        f"- packets: {report['summary']['packet_count']}",
        f"- documents: {report['summary']['document_count']}",
        f"- comparable documents: {report['summary'].get('comparable_document_count', report['summary']['document_count'])}",
        f"- expected receipt fields: {report['summary'].get('expected_field_count', 0)}",
        f"- matched receipt fields: {report['summary'].get('matched_field_count', 0)}",
        f"- grounded receipt docs: {report['summary'].get('grounding_document_count', 0)}",
        f"- docs with OCR geometry: {report['summary'].get('grounding_available_document_count', 0)}",
        f"- fully grounded docs: {report['summary'].get('grounding_fully_matched_document_count', 0)}",
        f"- expected grounded fields: {report['summary'].get('grounding_expected_field_count', 0)}",
        f"- matched grounded fields: {report['summary'].get('grounding_matched_field_count', 0)}",
        f"- pass comparisons: {report['summary'].get('pass_comparison_document_count', 0)}",
        f"- pass profile runs: {report['summary'].get('pass_comparison_profile_run_count', 0)}",
        f"- OCR inspections: {report['summary'].get('inspection_document_count', 0)}",
        f"- pass-comparison divergences: {report['summary'].get('pass_comparison_divergent_document_count', 0)}",
        f"- pass-comparison field disagreements: {report['summary'].get('pass_comparison_disagreement_count', 0)}",
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
    lines.append("## OCR Grounding")
    lines.append(
        f"- grounded receipt docs: {report['summary'].get('grounding_document_count', 0)}"
    )
    lines.append(
        f"- docs with OCR geometry: {report['summary'].get('grounding_available_document_count', 0)}"
    )
    lines.append(
        f"- fully grounded docs: {report['summary'].get('grounding_fully_matched_document_count', 0)}"
    )
    lines.append(
        f"- matched grounded fields: {report['summary'].get('grounding_matched_field_count', 0)}/{report['summary'].get('grounding_expected_field_count', 0)}"
    )
    grounding_counts = report["summary"].get("grounding_field_match_counts", {})
    if grounding_counts:
        for key, value in sorted(grounding_counts.items()):
            lines.append(f"- {key}: {value}")
    lines.append("")
    lines.append("## OCR Pass Comparison")
    confidence_counts = report["summary"].get("pass_comparison_confidence_counts", {})
    lines.append(
        f"- compared receipt docs: {report['summary'].get('pass_comparison_document_count', 0)}"
    )
    lines.append(
        f"- pass profile runs: {report['summary'].get('pass_comparison_profile_run_count', 0)}"
    )
    lines.append(
        f"- divergent receipt docs: {report['summary'].get('pass_comparison_divergent_document_count', 0)}"
    )
    lines.append(
        f"- total field disagreements: {report['summary'].get('pass_comparison_disagreement_count', 0)}"
    )
    if confidence_counts:
        for key, value in sorted(confidence_counts.items()):
            lines.append(f"- {key}: {value}")
    field_confidence_counts = report["summary"].get(
        "pass_comparison_field_confidence_counts", {}
    )
    if field_confidence_counts:
        lines.append("- field confidence counts:")
        for key, value in sorted(field_confidence_counts.items()):
            lines.append(f"  - {key}: {value}")
    field_status_counts = report["summary"].get("pass_comparison_field_status_counts", {})
    if field_status_counts:
        lines.append("- field status counts:")
        for key, value in sorted(field_status_counts.items()):
            lines.append(f"  - {key}: {value}")
    profile_summaries = report["summary"].get("pass_comparison_profile_summaries", {})
    if profile_summaries:
        lines.append("- profile summaries:")
        for profile_name, profile in sorted(profile_summaries.items()):
            confidence_counts = ", ".join(
                f"{key}={value}"
                for key, value in sorted(profile.get("confidence_counts", {}).items())
            )
            lines.append(
                f"  - {profile_name}: runs={profile.get('run_count', 0)}, "
                f"divergent_runs={profile.get('divergent_run_count', 0)}, "
                f"field_disagreements={profile.get('disagreement_count', 0)}, "
                f"confidence={confidence_counts or 'none'}"
            )
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
        "# OCR Model Comparison",
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
            f"fields={model.get('matched_field_count', 0)}/{model.get('expected_field_count', 0)}, "
            f"grounded={model.get('grounding_matched_field_count', 0)}/{model.get('grounding_expected_field_count', 0)}, "
            f"pass_disagreements={model.get('pass_comparison_disagreement_count', 0)}, "
            f"exact={model['exact_match_count']}, "
            f"relaxed={model['relaxed_match_count']}, "
            f"content={model['content_match_count']}"
        )

    lines.append("")
    lines.append("## OCR Grounding")
    for model in comparison["models"]:
        if model.get("status") == "error":
            lines.append(f"- {model['model']}: unavailable")
            continue
        field_counts = ", ".join(
            f"{key}={value}"
            for key, value in sorted(model.get("grounding_field_match_counts", {}).items())
        )
        lines.append(
            f"- {model['model']}: grounded_docs={model.get('grounding_document_count', 0)}, "
            f"geometry_docs={model.get('grounding_available_document_count', 0)}, "
            f"fully_grounded_docs={model.get('grounding_fully_matched_document_count', 0)}, "
            f"grounded_fields={model.get('grounding_matched_field_count', 0)}/{model.get('grounding_expected_field_count', 0)}, "
            f"field_matches={field_counts or 'none'}"
        )

    lines.append("")
    lines.append("## OCR Pass Comparison")
    for model in comparison["models"]:
        if model.get("status") == "error":
            lines.append(f"- {model['model']}: unavailable")
            continue
        confidence_counts = ", ".join(
            f"{key}={value}"
            for key, value in sorted(model.get("pass_comparison_confidence_counts", {}).items())
        )
        lines.append(
            f"- {model['model']}: compared_docs={model.get('pass_comparison_document_count', 0)}, "
            f"profile_runs={model.get('pass_comparison_profile_run_count', 0)}, "
            f"divergent_docs={model.get('pass_comparison_divergent_document_count', 0)}, "
            f"field_disagreements={model.get('pass_comparison_disagreement_count', 0)}, "
            f"confidence={confidence_counts or 'none'}"
        )
        for profile_name, profile in sorted(
            model.get("pass_comparison_profile_summaries", {}).items()
        ):
            profile_confidence = ", ".join(
                f"{key}={value}"
                for key, value in sorted(profile.get("confidence_counts", {}).items())
            )
            lines.append(
                f"  - {profile_name}: runs={profile.get('run_count', 0)}, "
                f"divergent_runs={profile.get('divergent_run_count', 0)}, "
                f"field_disagreements={profile.get('disagreement_count', 0)}, "
                f"confidence={profile_confidence or 'none'}"
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


def render_model_report_html(report: dict) -> str:
    summary = report["summary"]
    html = [
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">",
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
        "<title>OCR Evaluation</title>",
        "<style>",
        ":root{color-scheme:light;font-family:ui-sans-serif,system-ui,sans-serif;}",
        "body{margin:0;background:#f7f3eb;color:#231f1a;}",
        "main{max-width:1240px;margin:0 auto;padding:32px 24px 48px;}",
        "h1,h2{margin:0 0 12px;}",
        ".panel{background:#fffdf9;border:1px solid #ddcfbb;border-radius:18px;box-shadow:0 8px 24px rgba(86,61,35,.08);padding:20px 22px;margin-bottom:18px;}",
        ".grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px;}",
        ".metric{padding:12px 14px;border-radius:14px;background:#f3ece0;border:1px solid #e1d3bf;}",
        ".metric-label{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#8a5a2b;font-weight:700;}",
        ".metric-value{margin-top:6px;font-size:20px;font-weight:700;}",
        ".muted{color:#6f6258;}",
        ".packet{border-top:1px solid #eadfce;padding-top:14px;margin-top:14px;}",
        ".packet:first-child{border-top:0;padding-top:0;margin-top:0;}",
        "table{width:100%;border-collapse:collapse;margin-top:12px;}",
        "th,td{text-align:left;padding:10px 8px;border-top:1px solid #eadfce;vertical-align:top;}",
        "th{font-size:12px;text-transform:uppercase;letter-spacing:.05em;color:#8a5a2b;}",
        "a{color:#8a5a2b;font-weight:700;text-decoration:none;}",
        ".badge{display:inline-flex;align-items:center;padding:4px 10px;border-radius:999px;font-size:12px;font-weight:700;text-transform:uppercase;letter-spacing:.05em;border:1px solid rgba(120,97,73,.16);background:#fff;}",
        ".ok{background:#edf5f1;color:#376647;border-color:rgba(55,102,71,.2);}",
        ".warn{background:#f8f0dd;color:#91691f;border-color:rgba(171,127,44,.2);}",
        ".bad{background:#fbebe4;color:#9c4f32;border-color:rgba(182,108,63,.2);}",
        "</style></head><body><main>",
    ]
    html.append("<section class=\"panel\">")
    html.append("<p class=\"muted\">Corpus OCR evaluation</p>")
    html.append(f"<h1>{escape_html(summary['model'])}</h1>")
    html.append("<div class=\"grid\">")
    for label, value in [
        ("status", summary.get("status", "ok")),
        ("documents", summary["document_count"]),
        ("exact matches", summary["exact_match_count"]),
        ("grounded fields", f"{summary.get('grounding_matched_field_count', 0)}/{summary.get('grounding_expected_field_count', 0)}"),
        ("pass disagreements", summary.get("pass_comparison_disagreement_count", 0)),
        ("pass profile runs", summary.get("pass_comparison_profile_run_count", 0)),
        ("OCR inspections", summary.get("inspection_document_count", 0)),
    ]:
        html.append(
            f"<div class=\"metric\"><div class=\"metric-label\">{escape_html(str(label))}</div><div class=\"metric-value\">{escape_html(str(value))}</div></div>"
        )
    html.append("</div></section>")

    if summary.get("error"):
        html.append("<section class=\"panel\">")
        html.append("<h2>Error</h2>")
        html.append(f"<p>{escape_html(summary['error'])}</p>")
        html.append("</section></main></body></html>")
        return "".join(html)

    profile_summaries = summary.get("pass_comparison_profile_summaries", {})
    if profile_summaries:
        html.append("<section class=\"panel\"><h2>OCR Pass Profile Breakdown</h2>")
        html.append(
            "<table><thead><tr><th>Profile</th><th>Runs</th><th>Divergent Runs</th><th>Field Disagreements</th><th>Confidence</th></tr></thead><tbody>"
        )
        for profile_name, profile in sorted(profile_summaries.items()):
            confidence_counts = ", ".join(
                f"{key}={value}"
                for key, value in sorted(profile.get("confidence_counts", {}).items())
            )
            html.append("<tr>")
            html.append(f"<td>{escape_html(profile_name)}</td>")
            html.append(f"<td>{escape_html(profile.get('run_count', 0))}</td>")
            html.append(
                f"<td>{escape_html(profile.get('divergent_run_count', 0))}</td>"
            )
            html.append(
                f"<td>{escape_html(profile.get('disagreement_count', 0))}</td>"
            )
            html.append(
                f"<td>{escape_html(confidence_counts or 'none')}</td>"
            )
            html.append("</tr>")
        html.append("</tbody></table></section>")

    html.append("<section class=\"panel\"><h2>Packet Results</h2>")
    for packet in report["packets"]:
        readiness = packet["readiness"]
        html.append("<div class=\"packet\">")
        html.append(
            f"<p><strong>{escape_html(packet['packet_id'])}</strong> <span class=\"badge {'bad' if packet['readiness']['automation_gap_count'] else 'ok'}\">{escape_html(packet['filing_status'])}</span></p>"
        )
        html.append(
            f"<p class=\"muted\">automation_gaps={readiness['automation_gap_count']} · user_input_gaps={readiness['user_input_gap_count']} · manual_review_items={readiness['manual_review_item_count']}</p>"
        )
        html.append("<table><thead><tr><th>Document</th><th>Kind</th><th>Markdown</th><th>Fields</th><th>Grounding</th><th>Passes</th><th>Artifacts</th></tr></thead><tbody>")
        for document in packet["documents"]:
            expected_fields = document.get("expected_fields") or {}
            expected_grounding = document.get("expected_grounding") or {}
            pass_comparison = (document.get("ocr_pass_comparison") or {}).get("comparison") or {}
            artifact_links = []
            for label, path in [
                ("transcription", document.get("transcription_json")),
                ("facts", document.get("facts_json")),
                ("inspection", document.get("ocr_inspection_html")),
                ("grounding", document.get("ocr_grounding_html")),
            ]:
                if path:
                    artifact_links.append(
                        f"<a href=\"file://{escape_html_attribute(path)}\" target=\"_blank\" rel=\"noreferrer noopener\">{escape_html(label)}</a>"
                    )
            pass_profiles = document.get("ocr_pass_comparison_profiles") or []
            if pass_profiles:
                for profile in pass_profiles:
                    if profile.get("html_path"):
                        label = f"pass diff ({profile['name']})"
                        artifact_links.append(
                            f"<a href=\"file://{escape_html_attribute(profile['html_path'])}\" target=\"_blank\" rel=\"noreferrer noopener\">{escape_html(label)}</a>"
                        )
            elif document.get("ocr_pass_comparison_html"):
                artifact_links.append(
                    f"<a href=\"file://{escape_html_attribute(document['ocr_pass_comparison_html'])}\" target=\"_blank\" rel=\"noreferrer noopener\">pass diff</a>"
                )
            html.append("<tr>")
            html.append(f"<td>{escape_html(document.get('document_id') or document['input_document'])}</td>")
            html.append(f"<td>{escape_html(document['kind'])}</td>")
            html.append(
                f"<td>{escape_html(render_match_summary(document.get('exact_match'), document.get('relaxed_match'), document.get('content_match')))}</td>"
            )
            html.append(
                f"<td>{escape_html(render_field_match_summary(expected_fields.get('matched_field_count'), expected_fields.get('expected_field_count')))}</td>"
            )
            html.append(
                f"<td>{escape_html(render_field_match_summary(expected_grounding.get('matched_field_count'), expected_grounding.get('expected_field_count')))}</td>"
            )
            html.append(
                f"<td>{escape_html(render_pass_summary(pass_comparison, pass_profiles))}</td>"
            )
            html.append(f"<td>{' · '.join(artifact_links) or '<span class=\"muted\">none</span>'}</td>")
            html.append("</tr>")
        html.append("</tbody></table></div>")
    html.append("</section></main></body></html>")
    return "".join(html)


def render_match_summary(exact_match, relaxed_match, content_match) -> str:
    if exact_match is None:
        return "n/a"
    return f"exact={str(bool(exact_match)).lower()}, relaxed={str(bool(relaxed_match)).lower()}, content={str(bool(content_match)).lower()}"


def render_field_match_summary(matched_count, expected_count) -> str:
    if matched_count is None or expected_count is None:
        return "n/a"
    return f"{matched_count}/{expected_count}"


def render_pass_summary(pass_comparison: dict, pass_profiles: list[dict] | None = None) -> str:
    if pass_profiles:
        parts = []
        for profile in pass_profiles:
            summary = profile.get("summary") or {}
            confidence = summary.get("overall_confidence") or "unknown"
            disagreements = summary.get("disagreement_count", 0)
            parts.append(f"{profile['name']}={confidence}/{disagreements}")
        return "; ".join(parts) or "n/a"
    if not pass_comparison:
        return "n/a"
    confidence = pass_comparison.get("overall_confidence") or "unknown"
    disagreements = pass_comparison.get("disagreement_count", 0)
    return f"{confidence}, disagreements={disagreements}"


def escape_html(value: str) -> str:
    return (
        str(value)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&#39;")
    )


def escape_html_attribute(value: str) -> str:
    return escape_html(value)


def write_reports(output_dir: Path, model_reports: list[dict]) -> None:
    comparison = summarize_comparison(model_reports)
    write_json(output_dir / "ocr_comparison.json", comparison)
    (output_dir / "ocr_comparison.md").write_text(render_comparison_markdown(comparison))

    if len(model_reports) == 1:
        write_json(output_dir / "ocr_evaluation.json", model_reports[0])
        (output_dir / "ocr_evaluation.md").write_text(
            render_model_report_markdown(model_reports[0])
        )
        (output_dir / "ocr_evaluation.html").write_text(
            render_model_report_html(model_reports[0])
        )

    for report in model_reports:
        model_key = report["summary"]["model_key"]
        write_json(output_dir / f"ocr_evaluation_{model_key}.json", report)
        (output_dir / f"ocr_evaluation_{model_key}.md").write_text(
            render_model_report_markdown(report)
        )
        (output_dir / f"ocr_evaluation_{model_key}.html").write_text(
            render_model_report_html(report)
        )


def main() -> int:
    args = parse_args()
    try:
        compare_profiles = resolve_compare_profiles(args) if args.compare_passes else []
    except ValueError as err:
        print(f"error: {err}", flush=True)
        return 1
    models = resolve_models(args)
    output_dir = Path(args.output_dir)
    source_dir = output_dir / "source_corpus"
    rendered_dir = output_dir / "rendered"
    ingestion_dir = output_dir / "ingestion"
    output_dir.mkdir(parents=True, exist_ok=True)

    if args.corpus_manifest:
        corpus_spec = build_manifest_corpus_spec(Path(args.corpus_manifest))
    else:
        manifest = prepare_source_corpus(source_dir, args.packets)
        rendered_paths_by_packet = render_corpus_documents(manifest, source_dir, rendered_dir)
        corpus_spec = build_synthetic_corpus_spec(
            manifest,
            source_dir,
            rendered_paths_by_packet,
        )

    model_reports = []
    for model in models:
        try:
            if args.compare_passes:
                args.compare_profiles = [
                    ":".join(
                        [
                            profile["name"],
                            profile["pass_kind"],
                            profile["preprocess_variant"],
                        ]
                    )
                    for profile in compare_profiles
                ]
            model_reports.append(
                evaluate_model(
                    corpus_spec,
                    ingestion_dir,
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
