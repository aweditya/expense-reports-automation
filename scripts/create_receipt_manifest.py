#!/usr/bin/env python3

import argparse
import json
from pathlib import Path


def parse_expected_field(raw: str) -> tuple[str, str]:
    if "=" not in raw:
        raise ValueError(
            f"invalid expected field {raw!r}; expected key=value"
        )
    key, value = raw.split("=", 1)
    key = key.strip()
    value = value.strip()
    if not key:
        raise ValueError("expected field key must not be empty")
    if not value:
        raise ValueError("expected field value must not be empty")
    return key, value


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
    return identifier or "receipt"


def build_manifest(args: argparse.Namespace) -> dict:
    expected_fields = dict(parse_expected_field(raw) for raw in (args.expected_field or []))
    document_id = args.document_id or sanitize_identifier(args.input.stem)
    packet_id = args.packet_id or document_id
    return {
        "corpus_name": args.corpus_name or f"local_{document_id}",
        "documents": [
            {
                "document_id": document_id,
                "packet_id": packet_id,
                "kind": "receipt",
                "source_name": args.source_name,
                "source_url": args.source_url,
                "license": args.license_name,
                "input_path": str(args.input.resolve()),
                "expected_fields": expected_fields,
                "tags": args.tag or ["local", "receipt"],
            }
        ],
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Create a one-document receipt corpus manifest for a local receipt image "
            "so it can be evaluated by the OCR corpus harness."
        )
    )
    parser.add_argument("--input", type=Path, required=True, help="Path to the local receipt image or PDF")
    parser.add_argument("--output", type=Path, required=True, help="Where to write the manifest JSON")
    parser.add_argument("--document-id", help="Stable document identifier; defaults to a sanitized stem")
    parser.add_argument("--packet-id", help="Packet identifier; defaults to document id")
    parser.add_argument("--corpus-name", help="Top-level corpus name; defaults to local_<document_id>")
    parser.add_argument(
        "--expected-field",
        action="append",
        help="Expected field in key=value form. Repeat for merchant_name, transaction_date, total_paid, etc.",
    )
    parser.add_argument("--source-name", default="Local Receipt", help="Human-readable source label")
    parser.add_argument("--source-url", default="", help="Optional provenance URL")
    parser.add_argument("--license", dest="license_name", default="private/local", help="Source license label")
    parser.add_argument("--tag", action="append", help="Optional manifest tag. Repeat for multiple tags.")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.input.exists():
        raise SystemExit(f"input file not found: {args.input}")
    manifest = build_manifest(args)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(manifest, indent=2))
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
