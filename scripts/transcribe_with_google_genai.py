#!/usr/bin/env python3

import argparse
import io
import json
import time
from pathlib import Path


DEFAULT_MODEL = "gemini-3-flash-preview"
CLOUD_PLATFORM_SCOPE = "https://www.googleapis.com/auth/cloud-platform"
DEFAULT_GENERATE_RETRIES = 3
GROUNDABLE_IMAGE_MIME_TYPES = {"image/png", "image/jpeg"}


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
        "--pass-id",
        help="Stable OCR pass identifier to include in the emitted artifact metadata",
    )
    parser.add_argument(
        "--pass-kind",
        default="primary",
        help="OCR pass kind metadata, for example primary, verification, or table_focused",
    )
    parser.add_argument(
        "--preprocess-variant",
        default="original",
        help="Preprocessing variant metadata, for example original, contrast_boosted, or binarized",
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


def build_prompt(filename: str, mime_type: str, pass_kind: str) -> str:
    prompt = (
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
    if pass_kind == "table_focused":
        prompt += (
            "Additional instructions for this OCR pass:\n"
            "- Prioritize preserving table rows, aligned amounts, and totals exactly.\n"
            "- Keep item descriptions attached to their amount columns whenever possible.\n"
            "- Do not collapse nearby rows if they appear visually distinct.\n"
        )
    elif pass_kind == "verification":
        prompt += (
            "Additional instructions for this OCR pass:\n"
            "- Prioritize exactness for merchant name, date, currency, and total.\n"
            "- If text is faint or ambiguous, preserve the most faithful literal transcription.\n"
        )
    return prompt


def preprocess_document_bytes(
    document_path: Path, file_bytes: bytes, mime_type: str, preprocess_variant: str
) -> tuple[bytes, str]:
    if preprocess_variant == "original" or mime_type == "application/pdf":
        return file_bytes, mime_type

    if mime_type not in {"image/png", "image/jpeg"}:
        return file_bytes, mime_type

    try:
        from PIL import Image, ImageEnhance
    except ImportError as exc:  # pragma: no cover - dependency exists in repo venv
        raise SystemExit(
            f"Pillow is required for preprocess variant {preprocess_variant}: {exc}"
        ) from exc

    image = Image.open(io.BytesIO(file_bytes))
    image.load()

    if preprocess_variant == "contrast_boosted":
        image = ImageEnhance.Contrast(image).enhance(1.8)
    elif preprocess_variant == "grayscale":
        image = image.convert("L")
    elif preprocess_variant == "binarized":
        grayscale = image.convert("L")
        image = grayscale.point(lambda value: 255 if value >= 170 else 0, mode="1")
    elif preprocess_variant == "deskewed":
        # Real deskewing can be added later; keep this variant explicit in metadata now.
        image = image
    else:
        raise SystemExit(f"unsupported preprocess variant: {preprocess_variant}")

    output = io.BytesIO()
    rendered = image.convert("L") if image.mode == "1" else image
    rendered.save(output, format="PNG")
    return output.getvalue(), "image/png"


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
                "dimensions": None,
                "regions": [],
            }
        )
    return normalized


def build_grounding_prompt(filename: str, page_text: str) -> str:
    return (
        "Locate a small set of key receipt fields in this financial document image and return only JSON.\n"
        "Use this schema exactly:\n"
        "{\"regions\":[{\"region_id\":\"merchant_name\",\"kind\":\"value_candidate\",\"text\":\"...\",\"box_2d\":[y_min,x_min,y_max,x_max]}]}\n"
        "Rules:\n"
        "- Only include regions you can localize confidently.\n"
        "- Use region_id values only from this set when applicable: merchant_name, transaction_date, total_paid, total_paid_currency.\n"
        "- box_2d must use normalized 0-1000 coordinates in [y_min, x_min, y_max, x_max] order.\n"
        "- Keep text faithful to the visible document.\n"
        "- Prefer the final payable total, not subtotal or tax, for total_paid.\n"
        f"Filename: {filename}\n"
        "OCR transcription for context:\n"
        f"{page_text[:2500]}\n"
    )


def normalize_grounding_regions(payload: dict) -> list[dict]:
    raw_regions = payload.get("regions") if isinstance(payload, dict) else payload
    if not isinstance(raw_regions, list):
        return []

    normalized = []
    for index, region in enumerate(raw_regions, start=1):
        if not isinstance(region, dict):
            continue
        box = region.get("box_2d")
        if not isinstance(box, list) or len(box) != 4:
            continue
        try:
            y_min, x_min, y_max, x_max = [float(value) for value in box]
        except (TypeError, ValueError):
            continue
        if y_max < y_min or x_max < x_min:
            continue
        region_id = sanitize_identifier(region.get("region_id") or f"region_{index}")
        text = str(region.get("text") or "").strip()
        kind = str(region.get("kind") or "value_candidate").strip() or "value_candidate"
        normalized.append(
            {
                "region_id": region_id,
                "kind": kind,
                "text": text,
                "bbox": {
                    "left": x_min / 1000.0,
                    "top": y_min / 1000.0,
                    "width": (x_max - x_min) / 1000.0,
                    "height": (y_max - y_min) / 1000.0,
                },
            }
        )
    return normalized


def image_page_dimensions(file_bytes: bytes, mime_type: str) -> dict | None:
    if mime_type not in GROUNDABLE_IMAGE_MIME_TYPES:
        return None
    try:
        from PIL import Image
    except ImportError as exc:  # pragma: no cover - dependency exists in repo venv
        raise SystemExit(f"Pillow is required to inspect image dimensions: {exc}") from exc

    image = Image.open(io.BytesIO(file_bytes))
    image.load()
    width, height = image.size
    return {"width": width, "height": height}


def maybe_ground_key_receipt_fields(
    client,
    *,
    model: str,
    filename: str,
    file_bytes: bytes,
    mime_type: str,
    normalized_pages: list[dict],
):
    if mime_type not in GROUNDABLE_IMAGE_MIME_TYPES or not normalized_pages:
        return normalized_pages, "none", False

    dimensions = image_page_dimensions(file_bytes, mime_type)
    enriched_pages = [dict(page) for page in normalized_pages]
    if dimensions:
        enriched_pages[0] = dict(enriched_pages[0])
        enriched_pages[0]["dimensions"] = dimensions

    prompt = build_grounding_prompt(filename, enriched_pages[0]["text"])
    try:
        from google.genai import types

        response = generate_content_with_retries(
            client,
            model=model,
            contents=[
                prompt,
                types.Part.from_bytes(data=file_bytes, mime_type=mime_type),
            ],
            config=types.GenerateContentConfig(
                temperature=0,
                response_mime_type="application/json",
                max_output_tokens=4096,
            ),
        )
        payload = json.loads(extract_payload_text(response))
        regions = normalize_grounding_regions(payload)
    except Exception:
        return enriched_pages, "none", False

    if regions:
        enriched_pages[0] = dict(enriched_pages[0])
        enriched_pages[0]["regions"] = regions
        return enriched_pages, "gemini", True
    return enriched_pages, "none", False


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


def generate_content_with_retries(
    client,
    *,
    model: str,
    contents,
    config,
    max_attempts: int = DEFAULT_GENERATE_RETRIES,
    sleep_fn=time.sleep,
):
    last_error = None
    for attempt in range(1, max_attempts + 1):
        try:
            return client.models.generate_content(
                model=model,
                contents=contents,
                config=config,
            )
        except Exception as exc:
            last_error = exc
            if attempt == max_attempts:
                raise
            sleep_fn(min(2 ** (attempt - 1), 4))

    raise last_error  # pragma: no cover


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
    file_bytes = document_path.read_bytes()
    file_bytes, mime_type = preprocess_document_bytes(
        document_path,
        file_bytes,
        mime_type,
        args.preprocess_variant,
    )
    prompt = build_prompt(document_path.name, mime_type, args.pass_kind)

    response = generate_content_with_retries(
        client,
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

    normalized_pages = normalize_pages(payload)
    normalized_pages, geometry_source, geometry_available = maybe_ground_key_receipt_fields(
        client,
        model=args.model,
        filename=document_path.name,
        file_bytes=file_bytes,
        mime_type=mime_type,
        normalized_pages=normalized_pages,
    )

    result = {
        "document_id": sanitize_identifier(document_path.stem),
        "filename": document_path.name,
        "source_path": str(document_path),
        "engine": "vertex_gemini_sdk",
        "metadata": {
            "pass_id": args.pass_id
            or f"{sanitize_identifier(document_path.stem)}_{args.pass_kind}_{args.preprocess_variant}",
            "pass_kind": args.pass_kind,
            "preprocess_variant": args.preprocess_variant,
            "producer": "google_genai_sdk",
            "model": args.model,
            "geometry_source": geometry_source,
            "geometry_available": geometry_available,
        },
        "pages": normalized_pages,
    }
    rendered = json.dumps(result, indent=2)

    if args.output:
        Path(args.output).write_text(rendered)
    else:
        print(rendered)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
