#!/usr/bin/env python3

import argparse
import json
from pathlib import Path


DATASETS = {
    "sroie": {
        "hf_name": "jsdnrs/ICDAR2019-SROIE",
        "default_split": "train",
        "license": "cc-by-4.0",
        "source_name": "ICDAR2019-SROIE",
        "source_url": "https://huggingface.co/datasets/jsdnrs/ICDAR2019-SROIE",
    },
    "korean_receipts": {
        "hf_name": "HumynLabs/Korean_Receipts_Dataset",
        "default_split": "train",
        "license": "cc-by-4.0",
        "source_name": "Korean_Receipts_Dataset",
        "source_url": "https://huggingface.co/datasets/HumynLabs/Korean_Receipts_Dataset",
    },
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Import a small public receipt dataset slice into a manifest-backed OCR corpus."
    )
    parser.add_argument(
        "--dataset",
        choices=sorted(DATASETS),
        default="sroie",
        help="Public receipt dataset slice to import",
    )
    parser.add_argument(
        "--output-dir",
        default="reference/receipt_corpus",
        help="Corpus root directory",
    )
    parser.add_argument(
        "--split",
        help="Dataset split to import; defaults to the dataset's default split",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=8,
        help="Maximum number of rows to import",
    )
    parser.add_argument(
        "--offset",
        type=int,
        default=0,
        help="Row offset within the selected split",
    )
    parser.add_argument(
        "--manifest-name",
        help="Optional output manifest filename; defaults to a dataset-derived name",
    )
    return parser.parse_args()


def load_hf_load_dataset():
    try:
        from datasets import load_dataset
    except ImportError as exc:
        raise SystemExit(
            "This script requires the `datasets` package. "
            "Install it with `pip install datasets pillow`."
        ) from exc
    return load_dataset


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
    return "".join(cleaned).strip("_") or "receipt"


def relative_to(path: Path, root: Path) -> str:
    return str(path.resolve().relative_to(root.resolve()))


def scalar_entity_value(value):
    if value is None:
        return None
    if isinstance(value, str):
        stripped = value.strip()
        return stripped or None
    if isinstance(value, list):
        for candidate in value:
            scalar = scalar_entity_value(candidate)
            if scalar:
                return scalar
        return None
    if isinstance(value, dict):
        for key in ("text", "value", "label"):
            scalar = scalar_entity_value(value.get(key))
            if scalar:
                return scalar
        return None
    return str(value).strip() or None


def sanitize_amount(value: str | None) -> str | None:
    if not value:
        return None
    cleaned = "".join(ch for ch in value if ch.isdigit() or ch in {".", ","}).replace(",", "")
    if not cleaned:
        return None
    if "." not in cleaned:
        return None
    return cleaned


def build_sroie_expected_fields(row: dict) -> dict:
    entities = row.get("entities") or {}
    expected_fields = {"classification_kind": "receipt"}

    merchant_name = scalar_entity_value(entities.get("company"))
    if merchant_name:
        expected_fields["merchant_name"] = merchant_name

    total_paid = sanitize_amount(scalar_entity_value(entities.get("total")))
    if total_paid:
        expected_fields["total_paid"] = total_paid

    transaction_date = scalar_entity_value(entities.get("date"))
    if transaction_date:
        expected_fields["transaction_date"] = transaction_date

    return expected_fields


def build_generic_expected_fields() -> dict:
    return {"classification_kind": "receipt"}


def save_image(image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    image.save(path)


def import_dataset(args: argparse.Namespace) -> Path:
    config = DATASETS[args.dataset]
    split = args.split or config["default_split"]
    corpus_root = Path(args.output_dir)
    assets_dir = corpus_root / "assets" / args.dataset
    manifests_dir = corpus_root / "manifests"
    manifests_dir.mkdir(parents=True, exist_ok=True)
    assets_dir.mkdir(parents=True, exist_ok=True)

    load_dataset = load_hf_load_dataset()
    dataset = load_dataset(config["hf_name"], split=split)
    selected = dataset.select(range(args.offset, min(args.offset + args.limit, len(dataset))))

    documents = []
    for index, row in enumerate(selected):
        image = row["image"]
        source_id = row.get("key") or row.get("receipt_id") or f"{split}_{args.offset + index}"
        document_id = sanitize_identifier(source_id)
        image_path = assets_dir / f"{document_id}.png"
        save_image(image, image_path)

        if args.dataset == "sroie":
            expected_fields = build_sroie_expected_fields(row)
        else:
            expected_fields = build_generic_expected_fields()

        documents.append(
            {
                "document_id": document_id,
                "packet_id": document_id,
                "kind": "receipt",
                "source_name": config["source_name"],
                "source_url": config["source_url"],
                "license": config["license"],
                "input_path": relative_to(image_path, corpus_root),
                "expected_fields": expected_fields,
                "tags": ["real", "receipt", args.dataset, split],
            }
        )

    manifest = {
        "corpus_name": f"{args.dataset}_{split}_{args.offset}_{len(documents)}",
        "documents": documents,
    }

    manifest_name = (
        args.manifest_name
        or f"{args.dataset}_{split}_{args.offset}_{len(documents)}.json"
    )
    manifest_path = manifests_dir / manifest_name
    manifest_path.write_text(json.dumps(manifest, indent=2))
    return manifest_path


def main() -> int:
    args = parse_args()
    manifest_path = import_dataset(args)
    print(manifest_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
