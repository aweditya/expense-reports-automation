#!/usr/bin/env python3
"""Probe Vertex's Schema validator against each generated response_schema.

Vertex's Schema validator is stricter than the SDK's client-side
`types.Schema.model_validate`. A schema that round-trips through the
SDK (and through `cargo test`'s deserialization fixtures) can still be
rejected by Vertex with an opaque 400 INVALID_ARGUMENT. This script
catches that class of bug locally — call it before deploying any
schema-changing commit.

The probe sends one tiny `generate_content` call per schema with a
trivial prompt and `max_output_tokens=50` — fast (<2 s per kind) and
costs roughly nothing.

Modes:

  default            Probe each `generated/response_schema_<kind>.json`.
                     Exits non-zero if any kind is rejected.

  --bisect-lodging   Diagnostic. Runs a mutation matrix on the lodging
                     schema to isolate which field/pattern triggers the
                     rejection. Output to stdout and
                     `.scratch/lodging_bisect.txt`. Always exits 0
                     (it's diagnostic).

Auth: needs `gcloud auth application-default login` (same as the
extractors). Pass project via `VERTEX_PROJECT_ID` env var.
"""

from __future__ import annotations

import argparse
import copy
import json
import os
from pathlib import Path
from typing import Callable

from google import genai
from google.genai import types


REPO_ROOT = Path(__file__).resolve().parent.parent
GENERATED_DIR = REPO_ROOT / "generated"
SCRATCH_DIR = REPO_ROOT / ".scratch"

KINDS = ["meal", "transport", "lodging"]
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

    `ok=False` covers both SDK-side validation rejection (Pydantic
    ValidationError) and Vertex-side rejection (genai.errors.ClientError
    400). The message preserves enough of the upstream error to tell
    them apart.
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


def probe_all_kinds(client: genai.Client) -> int:
    failures = 0
    for kind in KINDS:
        path = GENERATED_DIR / f"response_schema_{kind}.json"
        schema_dict = json.loads(path.read_text())
        ok, msg = probe(client, schema_dict)
        marker = "OK  " if ok else "FAIL"
        print(f"{marker}  {kind:10s}  {msg}")
        if not ok:
            failures += 1
    return 1 if failures else 0


def bisect_lodging(client: genai.Client) -> int:
    """Mutate the lodging schema in N ways and probe each.

    Diagnostic only — does not modify any committed file. The point
    is to isolate which structural feature of `lodging_details` (or
    its surrounding context) Vertex rejects.
    """
    base = json.loads((GENERATED_DIR / "response_schema_lodging.json").read_text())

    ld_fields = [
        "hotel_name",
        "location",
        "check_in_date",
        "check_out_date",
        "booking_method",
        "is_shared_lodging",
    ]

    def drop_field(field: str) -> Callable[[dict], None]:
        def fn(m: dict) -> None:
            del m["items"]["properties"]["lodging_details"]["properties"][field]
            m["items"]["properties"]["lodging_details"]["required"].remove(field)
        return fn

    def strip_descriptions(m: dict) -> None:
        ld = m["items"]["properties"]["lodging_details"]["properties"]
        for leaf in ld.values():
            leaf["properties"]["value"].pop("description", None)

    def relax_booking_method_enum(m: dict) -> None:
        bm_value = (
            m["items"]["properties"]["lodging_details"]
            ["properties"]["booking_method"]["properties"]["value"]
        )
        bm_value.pop("enum", None)

    def strip_meta_from_details(m: dict) -> None:
        ld_props = m["items"]["properties"]["lodging_details"]["properties"]
        for fname in list(ld_props.keys()):
            ld_props[fname] = ld_props[fname]["properties"]["value"]

    mutations: list[tuple[str, Callable[[dict], None]]] = [
        ("baseline (unchanged)", lambda m: None),
        *[(f"drop lodging_details.{f}", drop_field(f)) for f in ld_fields],
        ("strip descriptions in lodging_details values", strip_descriptions),
        ("booking_method: drop enum (bare string)", relax_booking_method_enum),
        ("strip _meta from all lodging_details leaves", strip_meta_from_details),
    ]

    SCRATCH_DIR.mkdir(parents=True, exist_ok=True)
    out_path = SCRATCH_DIR / "lodging_bisect.txt"
    lines: list[str] = []
    for label, mutate in mutations:
        m = copy.deepcopy(base)
        mutate(m)
        ok, msg = probe(client, m)
        marker = "OK  " if ok else "FAIL"
        line = f"{marker}  {label}"
        print(line)
        lines.append(line)

    out_path.write_text("\n".join(lines) + "\n")
    print(f"\nWrote {out_path.relative_to(REPO_ROOT)}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--bisect-lodging",
        action="store_true",
        help="Run the lodging_details mutation matrix instead of probing all kinds.",
    )
    args = parser.parse_args()

    client = get_client()
    if args.bisect_lodging:
        return bisect_lodging(client)
    return probe_all_kinds(client)


if __name__ == "__main__":
    raise SystemExit(main())
