#!/usr/bin/env python3
"""Shared infrastructure for per-kind Gemini extractor scripts.

Per-kind extractor scripts (`extract_meal.py`, `extract_transport.py`, …)
all share the same shape: parse CLI args, set up a Vertex AI Gemini
client via ADC, load an image, send one structured-output call, write
the parsed JSON. The only per-kind variation is the **prompt text** and
the **response_schema path** — both are passed in by the caller.

This module owns everything else so adding a new expense kind is a
~30-line script (prompt + schema path + one call into here), not a
~220-line copy-paste.

Auth is Application Default Credentials (ADC). On Cloud Run that's
the runtime service account via the metadata server. Locally:
    gcloud auth application-default login
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path


DEFAULT_MODEL = "gemini-3-flash-preview"
DEFAULT_LOCATION = "global"


def parse_args(description: str | None = None) -> argparse.Namespace:
    """Parse the standard per-kind extractor CLI.

    All per-kind extractors take the same flags: --image (input file),
    --output (where to write the JSON), and the GCP project/location/
    model knobs. Defaults pull from environment variables so Cloud Run's
    `--set-env-vars` configuration is the only place that needs to know
    project IDs.
    """
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("--image", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--project",
        default=os.environ.get("VERTEX_PROJECT_ID"),
        help="GCP project (defaults to $VERTEX_PROJECT_ID; Cloud Run sets it "
        "via --set-env-vars in deploy/cloudbuild.yaml).",
    )
    parser.add_argument(
        "--location",
        default=os.environ.get("VERTEX_LOCATION", DEFAULT_LOCATION),
    )
    parser.add_argument(
        "--model",
        default=os.environ.get("VERTEX_GEMINI_MODEL", DEFAULT_MODEL),
    )
    return parser.parse_args()


def detect_mime_type(path: Path) -> str:
    suffix = path.suffix.lower()
    return {
        ".pdf": "application/pdf",
        ".png": "image/png",
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
    }.get(suffix) or sys.exit(f"unsupported image format: {suffix}")


def _load_response_schema(path: Path):
    """Load a generated response_schema JSON file as a google-genai Schema."""
    from google.genai import types

    schema_dict = json.loads(path.read_text())
    return types.Schema.model_validate(schema_dict)


def run_extraction(
    args: argparse.Namespace,
    prompt: str,
    response_schema_path: Path,
) -> int:
    """One Gemini call → structured JSON → disk.

    `args` comes from `parse_args()`. `prompt` is the kind-specific
    instructions; `response_schema_path` points to
    `generated/response_schema_<kind>.json`. Returns the process exit
    code (0 on success, non-zero on failure with diagnostics written
    to disk for triage).
    """
    if not args.image.exists():
        sys.exit(f"image not found: {args.image}")

    if not args.project:
        sys.exit(
            "project required: pass --project or set $VERTEX_PROJECT_ID. "
            "Cloud Run gets this from --set-env-vars in deploy/cloudbuild.yaml."
        )

    from google import genai
    from google.genai import types

    # ADC: works on Cloud Run via the metadata server, and locally after
    # `gcloud auth application-default login`. The SDK picks credentials
    # up automatically when none are passed.
    client = genai.Client(
        vertexai=True, project=args.project, location=args.location
    )

    image_bytes = args.image.read_bytes()
    mime = detect_mime_type(args.image)

    response_schema = _load_response_schema(response_schema_path)

    response = client.models.generate_content(
        model=args.model,
        contents=[
            f"Filename: {args.image.name}\n\n{prompt}",
            types.Part.from_bytes(data=image_bytes, mime_type=mime),
        ],
        config=types.GenerateContentConfig(
            response_mime_type="application/json",
            response_schema=response_schema,
            # Gemini 3 includes "thinking" tokens in this budget. Tamarine
            # spent 7860 thinking tokens at 8192 = too small. After Stage 6
            # added confidence_reason on every leaf the output volume grew
            # again — uber1.pdf blew past 32768 mid-JSON. Bumping to 65536
            # leaves headroom for multi-page PDFs (Uber receipts are 2 pages,
            # future hotel folios will be longer).
            max_output_tokens=65536,
            temperature=0.0,
        ),
    )

    raw_text = response.text or ""
    args.output.parent.mkdir(parents=True, exist_ok=True)

    try:
        parsed = json.loads(raw_text)
    except json.JSONDecodeError as err:
        raw_path = args.output.with_suffix(args.output.suffix + ".raw.txt")
        err_path = args.output.with_suffix(args.output.suffix + ".error.txt")
        raw_path.write_text(raw_text)
        diag = {
            "json_decode_error": str(err),
            "raw_text_length": len(raw_text),
            "candidates": [
                {
                    "finish_reason": str(getattr(c, "finish_reason", None)),
                    "safety_ratings": [str(r) for r in (getattr(c, "safety_ratings", None) or [])],
                    "content_parts": len((getattr(c, "content", None) and c.content.parts) or []),
                }
                for c in (getattr(response, "candidates", None) or [])
            ],
            "prompt_feedback": str(getattr(response, "prompt_feedback", None)),
            "usage_metadata": str(getattr(response, "usage_metadata", None)),
        }
        err_path.write_text(json.dumps(diag, indent=2))
        print(f"FAILED to parse JSON. Raw -> {raw_path}, diag -> {err_path}", file=sys.stderr)
        return 1

    # Inject source_filename — system context, not extracted by Gemini.
    # Reduction reads this to populate ExpenseReport.transaction_lines[]
    # .common.source_document.filename.
    if isinstance(parsed, list):
        for entry in parsed:
            if isinstance(entry, dict):
                entry["source_filename"] = args.image.name
    elif isinstance(parsed, dict):
        parsed["source_filename"] = args.image.name

    args.output.write_text(json.dumps(parsed, indent=2, ensure_ascii=False))
    print(f"wrote {args.output}")
    return 0
