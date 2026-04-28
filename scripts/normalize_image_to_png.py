#!/usr/bin/env python3
"""Normalize an uploaded image into a PNG artifact for OCR."""

from __future__ import annotations

import argparse
from pathlib import Path

from transcribe_with_google_genai import canonicalize_document_bytes_for_ocr, detect_mime_type


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_path", type=Path)
    parser.add_argument("output_path", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    file_bytes = args.input_path.read_bytes()
    mime_type = detect_mime_type(args.input_path, file_bytes)
    normalized_bytes, normalized_mime = canonicalize_document_bytes_for_ocr(
        args.input_path,
        file_bytes,
        mime_type,
    )
    if normalized_mime != "image/png":
        raise SystemExit(
            f"image normalization produced unexpected mime type {normalized_mime!r}"
        )
    args.output_path.parent.mkdir(parents=True, exist_ok=True)
    args.output_path.write_bytes(normalized_bytes)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
