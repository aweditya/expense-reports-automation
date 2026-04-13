#!/usr/bin/env python3

import argparse
import json
from pathlib import Path

from google import genai
from google.genai import types
from google.oauth2 import service_account


DEFAULT_MODEL = "gemini-3-flash-preview"
CLOUD_PLATFORM_SCOPE = "https://www.googleapis.com/auth/cloud-platform"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Transcribe a PDF or image document into markdown JSON using the Google Gen AI SDK."
    )
    parser.add_argument("document", help="Path to the PDF/PNG/JPG document")
    parser.add_argument(
        "--service-account-key",
        required=True,
        help="Path to a Google Cloud service-account JSON key",
    )
    parser.add_argument("--project", help="Google Cloud project id; defaults to the key's project")
    parser.add_argument(
        "--location",
        default="global",
        help="Vertex AI location; Gemini 3 preview examples use global",
    )
    parser.add_argument(
        "--model",
        default=DEFAULT_MODEL,
        help="Gemini model id, for example gemini-3-flash-preview or gemini-3-pro-preview",
    )
    parser.add_argument(
        "--output",
        help="Optional output file for the rendered transcribed-document JSON",
    )
    return parser.parse_args()


def detect_mime_type(path: Path) -> str:
    extension = path.suffix.lower()
    if extension == ".pdf":
        return "application/pdf"
    if extension == ".png":
        return "image/png"
    if extension in {".jpg", ".jpeg"}:
        return "image/jpeg"
    raise SystemExit(f"unsupported document format: {path.suffix}")


def build_prompt(filename: str, mime_type: str) -> str:
    return (
        "Transcribe this financial document into extractor-friendly markdown and return only JSON.\n"
        "Use this schema exactly:\n"
        "{\"pages\":[{\"page_number\":1,\"text\":\"...markdown...\"}]}\n"
        "Rules:\n"
        "- Preserve amounts, currencies, dates, names, confirmation codes, and IDs exactly.\n"
        "- Do not invent values or infer missing fields.\n"
        "- Use markdown headings and bullet points.\n"
        "- Prefer `Label: Value` bullets for standalone facts.\n"
        "- For repeated rows, use one bullet per row or pipe-delimited lines.\n"
        "- Keep each pipe-delimited row on a single logical line; do not split one row into multiple bullets or lines.\n"
        "- Keep one `pages[]` entry per source page when page boundaries are visible; otherwise use a single page.\n"
        "- Keep the top heading faithful to the source document.\n"
        "- If the source is clearly a flight itinerary, hotel folio, or merchant/card receipt, normalize the layout so headings and labels stay easy to parse downstream.\n"
        f"Filename: {filename}\n"
        f"Mime type: {mime_type}\n"
    )


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
    return identifier or "document"


def extract_payload_text(response) -> str:
    if getattr(response, "text", None):
        return response.text

    if getattr(response, "candidates", None):
        for candidate in response.candidates:
            content = getattr(candidate, "content", None)
            parts = getattr(content, "parts", None) or []
            text = "\n".join(
                part.text for part in parts if getattr(part, "text", None)
            ).strip()
            if text:
                return text

    raise SystemExit("Gemini response did not contain text")


def normalize_pages(payload: dict) -> list[dict]:
    pages = payload.get("pages") or payload.get("document_markdown_pages")
    if not pages:
        text = payload.get("markdown") or payload.get("text")
        if text:
            pages = [{"page_number": 1, "text": text}]
    if not pages:
        raise SystemExit("Gemini response JSON did not contain pages or markdown/text")

    normalized = []
    for index, page in enumerate(pages, start=1):
        normalized.append(
            {
                "page_number": page.get("page_number") or index,
                "text": page["text"],
            }
        )
    return normalized


def main() -> int:
    args = parse_args()
    document_path = Path(args.document)
    if not document_path.exists():
        raise SystemExit(f"document does not exist: {document_path}")

    key_path = Path(args.service_account_key)
    key_data = json.loads(key_path.read_text())
    project_id = args.project or key_data.get("project_id")
    if not project_id:
        raise SystemExit("project id is required via --project or the service-account key")

    credentials = service_account.Credentials.from_service_account_file(
        str(key_path),
        scopes=[CLOUD_PLATFORM_SCOPE],
    )
    client = genai.Client(
        vertexai=True,
        project=project_id,
        location=args.location,
        credentials=credentials,
    )

    mime_type = detect_mime_type(document_path)
    prompt = build_prompt(document_path.name, mime_type)
    file_bytes = document_path.read_bytes()

    response = client.models.generate_content(
        model=args.model,
        contents=[
            prompt,
            types.Part.from_bytes(data=file_bytes, mime_type=mime_type),
        ],
        config=types.GenerateContentConfig(
            temperature=0,
            response_mime_type="application/json",
            max_output_tokens=16384,
        ),
    )
    payload = json.loads(extract_payload_text(response))

    result = {
        "document_id": sanitize_identifier(document_path.stem),
        "filename": document_path.name,
        "source_path": str(document_path),
        "engine": "vertex_gemini_sdk",
        "pages": normalize_pages(payload),
    }
    rendered = json.dumps(result, indent=2)

    if args.output:
        Path(args.output).write_text(rendered)
    else:
        print(rendered)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
