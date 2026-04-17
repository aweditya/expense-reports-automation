#!/usr/bin/env python3

import argparse
import json
from pathlib import Path


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
        "- For receipts, prefer `# Merchant Receipt` when the source has no clear title, and prefer sections like `## Purchase Summary`, `## Line Items`, and `## Totals` when they are visually evident.\n"
        "- For receipts, keep merchant name, date, subtotal, tax, tip, total, card, and authorization values on the same logical line as their labels whenever possible.\n"
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
                "text": normalize_extractor_markdown(page["text"]),
            }
        )
    return normalized


def normalize_extractor_markdown(text: str) -> str:
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").split("\n")]
    normalized = []
    index = 0

    while index < len(lines):
        line = lines[index]

        if (
            index + 2 < len(lines)
            and is_section_heading(line)
            and not lines[index + 1].strip()
            and starts_extractor_content(lines[index + 2])
        ):
            normalized.append(line)
            index += 2
            continue

        if (
            index + 1 < len(lines)
            and line.lstrip().startswith(("-", "*"))
            and line.rstrip().endswith("|")
            and is_pipe_row_continuation_line(lines[index + 1])
        ):
            normalized.append(f"{line.rstrip()} {normalize_bullet_prefix(lines[index + 1])}")
            index += 2
            continue

        merged_label_value = merge_split_receipt_label_value(line, lines, index)
        if merged_label_value is not None:
            normalized.append(merged_label_value)
            index += 2
            continue

        merged_item_amount = merge_split_receipt_item_amount(line, lines, index)
        if merged_item_amount is not None:
            normalized.append(merged_item_amount)
            index += 2
            continue

        normalized.append(line)
        index += 1

    collapsed = []
    previous_blank = False
    for line in normalized:
        is_blank = not line.strip()
        if is_blank and previous_blank:
            continue
        collapsed.append(line)
        previous_blank = is_blank

    while collapsed and not collapsed[0].strip():
        collapsed.pop(0)
    while collapsed and not collapsed[-1].strip():
        collapsed.pop()

    return "\n".join(collapsed)


def merge_split_receipt_label_value(line: str, lines: list[str], index: int) -> str | None:
    if index + 1 >= len(lines):
        return None

    current = normalize_bullet_prefix(line)
    next_line = normalize_bullet_prefix(lines[index + 1])
    if not current or not next_line:
        return None
    if is_section_heading(line) or is_section_heading(lines[index + 1]):
        return None
    if looks_like_label_value_line(next_line):
        return None

    label = canonical_receipt_label(current)
    if not label:
        return None
    if not looks_like_receipt_value(next_line):
        return None

    return rebuild_line_with_prefix(line, f"{label}: {next_line}")


def merge_split_receipt_item_amount(line: str, lines: list[str], index: int) -> str | None:
    if index + 1 >= len(lines):
        return None

    current = normalize_bullet_prefix(line)
    next_line = normalize_bullet_prefix(lines[index + 1])
    if not current or not next_line:
        return None
    if is_section_heading(line) or is_section_heading(lines[index + 1]):
        return None
    if "|" in current or ":" in current:
        return None
    if canonical_receipt_label(current):
        return None
    if not looks_like_money_value(next_line):
        return None
    if not any(character.isalpha() for character in current):
        return None

    return rebuild_line_with_prefix(line, f"{current} | {next_line}")


def is_section_heading(line: str) -> bool:
    stripped = line.lstrip()
    return stripped.startswith("##")


def starts_extractor_content(line: str) -> bool:
    stripped = line.lstrip()
    return stripped.startswith(("-", "*")) or ":" in stripped


def is_pipe_row_continuation_line(line: str) -> bool:
    stripped = normalize_bullet_prefix(line).lower()
    return stripped.startswith("taxes & fees:") or stripped.startswith(
        "taxes and fees:"
    )


def normalize_bullet_prefix(line: str) -> str:
    return line.lstrip().lstrip("-").lstrip("*").strip()


def rebuild_line_with_prefix(original_line: str, normalized_content: str) -> str:
    stripped = original_line.lstrip()
    indent = original_line[: len(original_line) - len(stripped)]
    if stripped.startswith("- "):
        return f"{indent}- {normalized_content}"
    if stripped.startswith("* "):
        return f"{indent}* {normalized_content}"
    return f"{indent}{normalized_content}"


def canonical_receipt_label(line: str) -> str | None:
    stripped = line.rstrip(":").strip()
    normalized = normalize_receipt_key(stripped)
    labels = {
        "merchant": "Merchant",
        "merchant name": "Merchant Name",
        "merchant location": "Merchant Location",
        "location": "Location",
        "transaction date": "Transaction Date",
        "date": "Date",
        "subtotal": "Subtotal",
        "tax": "Tax",
        "gst": "GST",
        "vat": "VAT",
        "tip": "Tip",
        "total": "Total",
        "total paid": "Total Paid",
        "amount paid": "Amount Paid",
        "card": "Card",
        "authorization code": "Authorization Code",
        "terminal id": "Terminal ID",
    }
    return labels.get(normalized)


def looks_like_label_value_line(line: str) -> bool:
    return ":" in line and canonical_receipt_label(line.split(":", 1)[0]) is not None


def looks_like_receipt_value(line: str) -> bool:
    if looks_like_money_value(line):
        return True
    if any(character.isdigit() for character in line):
        return True
    return any(character.isalpha() for character in line)


def looks_like_money_value(line: str) -> bool:
    tokens = line.replace("$", " $ ").split()
    if not tokens:
        return False

    last_token = tokens[-1].replace(",", "")
    if last_token.count(".") != 1:
        return False
    whole, fraction = last_token.split(".", 1)
    if not whole.isdigit() or not fraction.isdigit():
        return False

    if len(tokens) == 1:
        return True
    if tokens[0] == "$":
        return True
    return tokens[0].isalpha() and len(tokens[0]) == 3


def normalize_receipt_key(value: str) -> str:
    return " ".join(
        value.lower()
        .replace("/", " ")
        .replace("-", " ")
        .replace("_", " ")
        .split()
    )


def main() -> int:
    from google import genai
    from google.genai import types
    from google.oauth2 import service_account

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
