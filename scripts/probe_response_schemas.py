#!/usr/bin/env python3
"""Probe Vertex's Schema validator against each generated response_schema.

Vertex's Schema validator is stricter than the SDK's client-side
`types.Schema.model_validate`. A schema that round-trips through the
SDK (and through `cargo test`'s deserialization fixtures) can still be
rejected by Vertex with an opaque 400 INVALID_ARGUMENT. This script
catches that class of bug locally — call it before deploying any
schema-changing commit.

The probe sends one tiny `generate_content` call per schema file with a
trivial prompt and `max_output_tokens=50` — fast (<2 s per file) and
costs roughly nothing.

Multi-call kinds (lodging, today) emit multiple schema files; each is
probed independently here. If any fails the script exits non-zero,
which is the signal to fix the schema before pushing.

Auth: needs `gcloud auth application-default login` (same as the
extractors). Pass project via `VERTEX_PROJECT_ID` env var.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

from google import genai
from google.genai import types


REPO_ROOT = Path(__file__).resolve().parent.parent
GENERATED_DIR = REPO_ROOT / "generated"

# Each generated schema we expect Vertex to accept. Single-call kinds
# (meal, transport) have one file; multi-call kinds (lodging) have
# multiple. Matches `SCHEMAS_TO_GENERATE` in
# `scripts/generate_response_schema.py`.
SCHEMA_FILES = [
    "response_schema_meal.json",
    "response_schema_transport.json",
    "response_schema_lodging_main.json",
    "response_schema_lodging_extras.json",
]

MODEL = "gemini-3-flash-preview"


def get_client() -> genai.Client:
    project = os.environ.get("VERTEX_PROJECT_ID") or os.environ.get("GOOGLE_CLOUD_PROJECT")
    if not project:
        raise SystemExit(
            "Set VERTEX_PROJECT_ID (or GOOGLE_CLOUD_PROJECT) before running. "
            "Locally: `export VERTEX_PROJECT_ID=soe-agile-agents`."
        )
    return genai.Client(vertexai=True, project=project, location="global")


def probe(client: genai.Client, schema_dict: dict) -> tuple[bool, str]:
    """Send a minimal generate_content call. Returns (ok, message).

    `ok=False` covers both SDK-side rejection (Pydantic ValidationError)
    and Vertex-side rejection (ClientError 400). The message preserves
    enough of the upstream error to tell them apart.
    """
    try:
        schema = types.Schema.model_validate(schema_dict)
        client.models.generate_content(
            model=MODEL,
            contents=["test"],
            config=types.GenerateContentConfig(
                response_mime_type="application/json",
                response_schema=schema,
                max_output_tokens=50,
                temperature=0.0,
            ),
        )
        return True, "OK"
    except Exception as e:
        return False, f"{type(e).__name__}: {str(e)[:200]}"


def main() -> int:
    client = get_client()
    failures = 0
    for filename in SCHEMA_FILES:
        path = GENERATED_DIR / filename
        if not path.exists():
            print(f"FAIL  {filename}  (not found — run scripts/generate_response_schema.py)")
            failures += 1
            continue
        schema_dict = json.loads(path.read_text())
        ok, msg = probe(client, schema_dict)
        marker = "OK  " if ok else "FAIL"
        # Trim the "response_schema_" prefix and ".json" suffix for display.
        label = filename[len("response_schema_") : -len(".json")]
        print(f"{marker}  {label:30s}  {msg}")
        if not ok:
            failures += 1
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
