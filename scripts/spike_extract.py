#!/usr/bin/env python3
"""M4 spike: single Gemini call extracts a transaction line from one receipt.

Throwaway. No response_schema, no schema generator, no production polish.
Goal is to inspect what Gemini returns when prompted with our schema's
vocabulary plain-text. Outputs are hand-inspected.

Auth mirrors scripts/transcribe_with_google_genai.py — service-account key
either via --service-account-key or VERTEX_SERVICE_ACCOUNT_KEY.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path


CLOUD_PLATFORM_SCOPE = "https://www.googleapis.com/auth/cloud-platform"
DEFAULT_MODEL = "gemini-3-flash-preview"
DEFAULT_LOCATION = "global"


PROMPT = """You are extracting a single transaction line from a receipt for a
Stanford expense report. The receipt is attached as an image.

Return ONE JSON object matching the shape below. Every leaf VALUE field must
be wrapped as `{"value": <value>, "_meta": {...}}`. Only fill fields you can
read or confidently infer from the receipt; leave others as `{"value": null,
"_meta": {"confidence": "low", "evidence": [{"kind": "system_generated",
"origin": "not_present_in_receipt"}], "needs_review": true, "flags": []}}`.

The shape:

{
  "expense_kind": "<one of: airfare | lodging | ground_transport | conference_registration | meal | car_rental | gift | human_subject | other>",
  "common": {
    "date":                  {"value": "<YYYY-MM-DD>", "_meta": {...}},
    "line_amount_usd":       {"value": <number>,       "_meta": {...}},
    "original_currency":     {"value": "<ISO 4217 code or null if USD>", "_meta": {...}},
    "original_amount":       {"value": <number or null>, "_meta": {...}},
    "expense_type":          {"value": "<one of: business_meal | business_meal_with_alcohol | group_travel_meal | group_travel_meal_with_alcohol | airfare_domestic | airfare_foreign | lodging_domestic | lodging_foreign | ground_transportation_domestic | ground_transportation_foreign | conference_registration | car_rental | other_business_expense>", "_meta": {...}},
    "remarks":               {"value": "<short context>", "_meta": {...}},
    "country_of_activity":   {"value": "<country or null>", "_meta": {...}},
    "source_documents":      {"value": [{"filename": "<as given>", "document_type": "receipt"}], "_meta": {...}}
  },
  "meal_details": {
    "venue_name":             {"value": "<merchant name>", "_meta": {...}},
    "tip_amount":             {"value": <number or null>,   "_meta": {...}},
    "alcohol_amount":         {"value": <number or null>,   "_meta": {...}},
    "has_alcohol_on_receipt": {"value": <true or false>,    "_meta": {...}},
    "attendees":              {"value": null, "_meta": {...}},
    "meal_purpose":           {"value": null, "_meta": {...}}
  }
}

If the receipt is not a meal, omit `meal_details` and add the matching block
(`airfare_details`, `lodging_details`, etc.) instead.

The `_meta` block on each leaf must look like:
{
  "confidence": "<high | medium | low>",
  "evidence": [
    {
      "kind": "<document_span | document | system_generated>",
      "filename": "<source filename>",
      "page": 1,
      "quote": "<exact text quoted from the receipt>"
    }
  ],
  "needs_review": <true if the FA must verify, false otherwise>,
  "flags": []
}

Confidence is ordinal — high, medium, or low. NOT a probability.
For `attendees` and `meal_purpose`, mark needs_review: true (only the FA knows).

Return ONLY the JSON object. No markdown fence, no commentary.
"""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--service-account-key",
        type=Path,
        default=os.environ.get("VERTEX_SERVICE_ACCOUNT_KEY"),
        help="Service account key path (defaults to $VERTEX_SERVICE_ACCOUNT_KEY).",
    )
    parser.add_argument(
        "--project",
        default=os.environ.get("VERTEX_PROJECT_ID"),
        help="GCP project (defaults to $VERTEX_PROJECT_ID or the key's project_id).",
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


def main() -> int:
    args = parse_args()

    if not args.image.exists():
        sys.exit(f"image not found: {args.image}")
    if args.service_account_key is None or not Path(args.service_account_key).exists():
        sys.exit("service account key required (--service-account-key or VERTEX_SERVICE_ACCOUNT_KEY)")

    from google import genai
    from google.genai import types
    from google.oauth2 import service_account

    key_path = Path(args.service_account_key)
    project = args.project or json.loads(key_path.read_text()).get("project_id")
    if not project:
        sys.exit("project required (--project or project_id in the key)")

    credentials = service_account.Credentials.from_service_account_file(
        str(key_path), scopes=[CLOUD_PLATFORM_SCOPE]
    )
    client = genai.Client(
        vertexai=True, project=project, location=args.location, credentials=credentials
    )

    image_bytes = args.image.read_bytes()
    mime = detect_mime_type(args.image)

    response = client.models.generate_content(
        model=args.model,
        contents=[
            f"Filename: {args.image.name}\n\n{PROMPT}",
            types.Part.from_bytes(data=image_bytes, mime_type=mime),
        ],
        config=types.GenerateContentConfig(
            response_mime_type="application/json",
            # Gemini 3 includes "thinking" tokens in this budget. Tamarine
            # spent 7860 thinking tokens; 8192 is too small. Be generous.
            max_output_tokens=32768,
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

    args.output.write_text(json.dumps(parsed, indent=2, ensure_ascii=False))
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
