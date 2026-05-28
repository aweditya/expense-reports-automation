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

# Auto-discover every generated response_schema. Previously a hand-
# maintained list — drift between this list and SCHEMAS_TO_GENERATE
# in generate_response_schema.py meant newly-added schemas would
# not get probed unless explicitly listed. Globbing closes that gap
# so every future schema is gated automatically.
def discover_schema_files() -> list[str]:
    return sorted(p.name for p in GENERATED_DIR.glob("response_schema_*.json"))

MODEL = "gemini-3-flash-preview"

# Schemas we know exceed Vertex's property-count ceiling but haven't
# yet split (a la lodging) because no extractor / form-option wires
# them today. Surfaced by the probe but skipped for gate purposes so
# active schemas don't get bundled with already-known-broken ones.
# When you wire one of these, drop it from this dict + ship the split.
KNOWN_BROKEN: dict[str, str] = {}


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
    schema_files = discover_schema_files()
    if not schema_files:
        print("no generated/response_schema_*.json found — run "
              "scripts/generate_response_schema.py first")
        return 2
    print(f"# probing {len(schema_files)} schemas against Vertex {MODEL}")
    for filename in schema_files:
        path = GENERATED_DIR / filename
        # Trim the "response_schema_" prefix and ".json" suffix for display.
        label = filename[len("response_schema_") : -len(".json")]
        if filename in KNOWN_BROKEN:
            print(f"SKIP  {label:30s}  known-broken: {KNOWN_BROKEN[filename]}")
            continue
        if not path.exists():
            print(f"FAIL  {label:30s}  not found — run scripts/generate_response_schema.py")
            failures += 1
            continue
        schema_dict = json.loads(path.read_text())
        ok, msg = probe(client, schema_dict)
        marker = "OK  " if ok else "FAIL"
        print(f"{marker}  {label:30s}  {msg}")
        if not ok:
            failures += 1
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
