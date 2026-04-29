#!/usr/bin/env python3

import argparse
import io
import json
import time
from pathlib import Path


DEFAULT_MODEL = "gemini-3-flash-preview"
CLOUD_PLATFORM_SCOPE = "https://www.googleapis.com/auth/cloud-platform"
DEFAULT_GENERATE_RETRIES = 3
EMPTY_TRANSCRIPTION_RESPONSE_RETRIES = 3
FULL_VARIANT_TRANSCRIPTION_CYCLES = 2
GROUNDABLE_IMAGE_MIME_TYPES = {
    "image/png",
    "image/jpeg",
    "image/heic",
    "image/heif",
}
GROUNDING_FALLBACK_VARIANTS = ("contrast_boosted", "binarized", "grayscale")
HEIF_FILE_TYPE_BRANDS = {
    b"heic",
    b"heix",
    b"hevc",
    b"hevx",
    b"heim",
    b"heis",
    b"mif1",
    b"msf1",
}
LOCALIZATION_MIN_LONG_EDGE = 2500
LOCALIZATION_MIN_PIXEL_AREA = 3_000_000


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


def detect_mime_type(path: Path, file_bytes: bytes | None = None) -> str:
    if file_bytes:
        sniffed = sniff_mime_type_from_bytes(file_bytes)
        if sniffed:
            return sniffed
    extension = path.suffix.lower()
    if extension == ".pdf":
        return "application/pdf"
    if extension == ".png":
        return "image/png"
    if extension in {".jpg", ".jpeg"}:
        return "image/jpeg"
    if extension in {".heic", ".heif"}:
        return "image/heic"
    raise SystemExit(f"unsupported document format: {path.suffix}")


def sniff_mime_type_from_bytes(file_bytes: bytes) -> str | None:
    if file_bytes.startswith(b"%PDF-"):
        return "application/pdf"
    if file_bytes.startswith(b"\x89PNG\r\n\x1a\n"):
        return "image/png"
    if file_bytes.startswith(b"\xff\xd8\xff"):
        return "image/jpeg"
    if len(file_bytes) >= 12 and file_bytes[4:8] == b"ftyp":
        major_brand = file_bytes[8:12]
        if major_brand in HEIF_FILE_TYPE_BRANDS:
            return "image/heic"
    return None


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


def build_markdown_fallback_prompt(filename: str, mime_type: str, pass_kind: str) -> str:
    prompt = (
        "Transcribe this financial document into extractor-friendly markdown and return only markdown.\n"
        "Rules:\n"
        "- Preserve amounts, currencies, dates, names, confirmation codes, and IDs exactly.\n"
        "- Do not invent values or infer missing fields.\n"
        "- Use markdown headings and bullet points.\n"
        "- Prefer `Label: Value` bullets for standalone facts.\n"
        "- For repeated rows, use one bullet per row or pipe-delimited lines.\n"
        "- Keep each pipe-delimited row on a single logical line; do not split one row into multiple bullets or lines.\n"
        "- Keep the top heading faithful to the source document.\n"
        "- For receipts, prefer `# Merchant Receipt` when the source has no clear title, and prefer sections like `## Purchase Summary`, `## Line Items`, and `## Totals` when they are visually evident.\n"
        f"Filename: {filename}\n"
        f"Mime type: {mime_type}\n"
    )
    if pass_kind == "table_focused":
        prompt += (
            "Additional instructions for this OCR pass:\n"
            "- Prioritize preserving table rows, aligned amounts, and totals exactly.\n"
            "- Keep item descriptions attached to their amount columns whenever possible.\n"
        )
    elif pass_kind == "verification":
        prompt += (
            "Additional instructions for this OCR pass:\n"
            "- Prioritize exactness for merchant name, date, currency, and total.\n"
            "- If text is faint or ambiguous, preserve the most faithful literal transcription.\n"
        )
    return prompt


def build_receipt_localization_prompt(filename: str) -> str:
    return (
        "Locate the outer boundary of the primary receipt or financial document in this image and return only JSON.\n"
        "Use this schema exactly:\n"
        "{\"box_2d\":[y_min,x_min,y_max,x_max]}\n"
        "Rules:\n"
        "- box_2d must use normalized 0-1000 coordinates in [y_min, x_min, y_max, x_max] order.\n"
        "- Include the full visible receipt/document, not just the text body.\n"
        "- Exclude the desk, background, shadows, and surrounding scene when possible.\n"
        "- If the image is already tightly cropped to the receipt, return the full-image box.\n"
        "- If you cannot confidently identify one primary receipt/document, return {}.\n"
        f"Filename: {filename}\n"
    )


def register_optional_heif_support() -> None:
    try:
        import pillow_heif
    except ImportError:
        return
    pillow_heif.register_heif_opener()


def open_image_for_ocr(file_bytes: bytes):
    from PIL import Image, ImageOps

    register_optional_heif_support()
    image = Image.open(io.BytesIO(file_bytes))
    image.load()
    return ImageOps.exif_transpose(image)


def canonicalize_document_bytes_for_ocr(
    document_path: Path, file_bytes: bytes, mime_type: str
) -> tuple[bytes, str]:
    if mime_type == "application/pdf":
        return file_bytes, mime_type
    if mime_type not in GROUNDABLE_IMAGE_MIME_TYPES:
        return file_bytes, mime_type

    try:
        image = open_image_for_ocr(file_bytes)
    except Exception as exc:
        raise SystemExit(
            f"could not decode image upload {document_path.name!r}: {exc}"
        ) from exc

    output = io.BytesIO()
    image.save(output, format="PNG")
    return output.getvalue(), "image/png"


def preprocess_document_bytes(
    document_path: Path, file_bytes: bytes, mime_type: str, preprocess_variant: str
) -> tuple[bytes, str]:
    file_bytes, mime_type = canonicalize_document_bytes_for_ocr(
        document_path,
        file_bytes,
        mime_type,
    )
    if preprocess_variant == "original" or mime_type == "application/pdf":
        return file_bytes, mime_type

    if mime_type not in {"image/png", "image/jpeg"}:
        return file_bytes, mime_type

    try:
        from PIL import ImageEnhance
    except ImportError as exc:  # pragma: no cover - dependency exists in repo venv
        raise SystemExit(
            f"Pillow is required for preprocess variant {preprocess_variant}: {exc}"
        ) from exc

    image = open_image_for_ocr(file_bytes)

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


def ocr_retry_variants(primary_variant: str, mime_type: str) -> list[str]:
    variants = [primary_variant or "original"]
    if mime_type not in GROUNDABLE_IMAGE_MIME_TYPES:
        return variants
    for candidate in GROUNDING_FALLBACK_VARIANTS:
        if candidate not in variants:
            variants.append(candidate)
    return variants


def is_retryable_transcription_error(exc: BaseException) -> bool:
    if isinstance(exc, json.JSONDecodeError):
        return True
    if isinstance(exc, SystemExit):
        message = str(exc)
        return (
            "Gemini response did not contain text" in message
            or "Gemini response JSON did not contain pages or markdown/text" in message
        )
    return False


def normalize_pages(payload) -> list[dict]:
    if isinstance(payload, list):
        pages = payload
    elif isinstance(payload, dict):
        pages = payload.get("pages") or payload.get("document_markdown_pages")
        if not pages:
            text = payload.get("markdown") or payload.get("text")
            if text:
                pages = [{"page_number": 1, "text": text}]
    else:
        pages = None
    if not pages:
        raise SystemExit("Gemini response JSON did not contain pages or markdown/text")

    normalized = []
    for index, page in enumerate(pages, start=1):
        if isinstance(page, str):
            text = page
        elif isinstance(page, dict):
            text = (
                page.get("text")
                or page.get("markdown")
                or page.get("content")
                or ""
            )
        else:
            text = str(page)
        normalized.append(
            {
                "page_number": page.get("page_number") or index if isinstance(page, dict) else index,
                "text": normalize_extractor_markdown(text),
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


def normalize_localization_bbox(payload: dict) -> dict | None:
    if not isinstance(payload, dict):
        return None
    raw_box = payload.get("box_2d")
    if not isinstance(raw_box, list) or len(raw_box) != 4:
        return None
    try:
        y_min, x_min, y_max, x_max = [float(value) for value in raw_box]
    except (TypeError, ValueError):
        return None
    if y_max <= y_min or x_max <= x_min:
        return None
    return {
        "left": max(0.0, min(1.0, x_min / 1000.0)),
        "top": max(0.0, min(1.0, y_min / 1000.0)),
        "right": max(0.0, min(1.0, x_max / 1000.0)),
        "bottom": max(0.0, min(1.0, y_max / 1000.0)),
    }


def image_page_dimensions(file_bytes: bytes, mime_type: str) -> dict | None:
    if mime_type not in GROUNDABLE_IMAGE_MIME_TYPES:
        return None
    try:
        image = open_image_for_ocr(file_bytes)
    except ImportError as exc:  # pragma: no cover - dependency exists in repo venv
        raise SystemExit(f"Pillow is required to inspect image dimensions: {exc}") from exc
    width, height = image.size
    return {"width": width, "height": height}


def should_attempt_receipt_localization(dimensions: dict | None) -> bool:
    if not dimensions:
        return False
    width = int(dimensions.get("width") or 0)
    height = int(dimensions.get("height") or 0)
    return (
        max(width, height) >= LOCALIZATION_MIN_LONG_EDGE
        or width * height >= LOCALIZATION_MIN_PIXEL_AREA
    )


def attempt_receipt_localization(
    client,
    *,
    model: str,
    filename: str,
    file_bytes: bytes,
    mime_type: str,
    generate_fn=None,
    part_factory=None,
) -> dict | None:
    if generate_fn is None:
        generate_fn = generate_content_with_retries
    config = {
        "temperature": 0,
        "response_mime_type": "application/json",
        "max_output_tokens": 1024,
    }
    if part_factory is None:
        from google.genai import types

        part_factory = lambda data, detected_mime: types.Part.from_bytes(  # noqa: E731
            data=data, mime_type=detected_mime
        )
        config = types.GenerateContentConfig(**config)

    response = generate_fn(
        client,
        model=model,
        contents=[
            build_receipt_localization_prompt(filename),
            part_factory(file_bytes, mime_type),
        ],
        config=config,
    )
    payload = json.loads(extract_payload_text(response))
    return normalize_localization_bbox(payload)


def crop_image_bytes_to_bbox(
    document_path: Path,
    file_bytes: bytes,
    mime_type: str,
    bbox: dict,
    padding_ratio: float = 0.02,
) -> tuple[bytes, str]:
    if mime_type not in GROUNDABLE_IMAGE_MIME_TYPES:
        return file_bytes, mime_type

    image = open_image_for_ocr(file_bytes)
    width, height = image.size
    pad_x = round(width * padding_ratio)
    pad_y = round(height * padding_ratio)

    left = max(0, round(width * float(bbox["left"])) - pad_x)
    top = max(0, round(height * float(bbox["top"])) - pad_y)
    right = min(width, round(width * float(bbox["right"])) + pad_x)
    bottom = min(height, round(height * float(bbox["bottom"])) + pad_y)
    if right <= left or bottom <= top:
        return file_bytes, mime_type

    cropped = image.crop((left, top, right, bottom))
    output = io.BytesIO()
    cropped.save(output, format="PNG")
    return output.getvalue(), "image/png"


def maybe_localize_receipt_content(
    client,
    *,
    model: str,
    filename: str,
    document_path: Path,
    file_bytes: bytes,
    mime_type: str,
    generate_fn=None,
    part_factory=None,
) -> tuple[bytes, str, bool, dict | None]:
    if mime_type not in GROUNDABLE_IMAGE_MIME_TYPES:
        return file_bytes, mime_type, False, None
    dimensions = image_page_dimensions(file_bytes, mime_type)
    if not should_attempt_receipt_localization(dimensions):
        return file_bytes, mime_type, False, None

    try:
        bbox = attempt_receipt_localization(
            client,
            model=model,
            filename=filename,
            file_bytes=file_bytes,
            mime_type=mime_type,
            generate_fn=generate_fn,
            part_factory=part_factory,
        )
    except Exception:
        return file_bytes, mime_type, False, None

    if not bbox:
        return file_bytes, mime_type, False, None

    width = max(0.0, float(bbox["right"]) - float(bbox["left"]))
    height = max(0.0, float(bbox["bottom"]) - float(bbox["top"]))
    if width * height < 0.15:
        return file_bytes, mime_type, False, bbox
    if width >= 0.98 and height >= 0.98:
        return file_bytes, mime_type, False, bbox

    cropped_bytes, cropped_mime = crop_image_bytes_to_bbox(
        document_path,
        file_bytes,
        mime_type,
        bbox,
    )
    return cropped_bytes, cropped_mime, True, bbox


def grounding_retry_variants(primary_variant: str) -> list[str]:
    variants = [primary_variant or "original"]
    for candidate in GROUNDING_FALLBACK_VARIANTS:
        if candidate not in variants:
            variants.append(candidate)
    return variants


def attempt_transcription_pages(
    client,
    *,
    model: str,
    prompt: str,
    file_bytes: bytes,
    mime_type: str,
    generate_fn=None,
    part_factory=None,
) -> list[dict]:
    if generate_fn is None:
        generate_fn = generate_content_with_retries
    config = {
        "temperature": 0,
        "response_mime_type": "application/json",
        "max_output_tokens": 16384,
    }
    if part_factory is None:
        from google.genai import types

        part_factory = lambda data, detected_mime: types.Part.from_bytes(  # noqa: E731
            data=data, mime_type=detected_mime
        )
        config = types.GenerateContentConfig(**config)

    last_error: BaseException | None = None
    for _ in range(EMPTY_TRANSCRIPTION_RESPONSE_RETRIES):
        response = generate_fn(
            client,
            model=model,
            contents=[
                prompt,
                part_factory(file_bytes, mime_type),
            ],
            config=config,
        )
        try:
            payload = json.loads(extract_payload_text(response))
            return normalize_pages(payload)
        except KeyboardInterrupt:
            raise
        except BaseException as exc:
            if not is_retryable_transcription_error(exc):
                raise
            last_error = exc

    if isinstance(last_error, SystemExit):
        raise last_error
    if last_error is not None:
        raise SystemExit(f"Gemini transcription did not stabilize: {last_error}") from last_error
    raise SystemExit("Gemini transcription did not stabilize")


def attempt_markdown_transcription_pages(
    client,
    *,
    model: str,
    prompt: str,
    file_bytes: bytes,
    mime_type: str,
    generate_fn=None,
    part_factory=None,
) -> list[dict]:
    if generate_fn is None:
        generate_fn = generate_content_with_retries
    config = {
        "temperature": 0,
        "response_mime_type": "text/plain",
        "max_output_tokens": 16384,
    }
    if part_factory is None:
        from google.genai import types

        part_factory = lambda data, detected_mime: types.Part.from_bytes(  # noqa: E731
            data=data, mime_type=detected_mime
        )
        config = types.GenerateContentConfig(**config)

    last_error: BaseException | None = None
    for _ in range(EMPTY_TRANSCRIPTION_RESPONSE_RETRIES):
        response = generate_fn(
            client,
            model=model,
            contents=[
                prompt,
                part_factory(file_bytes, mime_type),
            ],
            config=config,
        )
        try:
            text = extract_payload_text(response).strip()
            if not text:
                raise SystemExit("Gemini response did not contain text")
            return [
                {
                    "page_number": 1,
                    "text": normalize_extractor_markdown(text),
                    "dimensions": None,
                    "regions": [],
                }
            ]
        except KeyboardInterrupt:
            raise
        except BaseException as exc:
            if not is_retryable_transcription_error(exc):
                raise
            last_error = exc

    if isinstance(last_error, SystemExit):
        raise last_error
    if last_error is not None:
        raise SystemExit(
            f"Gemini markdown transcription did not stabilize: {last_error}"
        ) from last_error
    raise SystemExit("Gemini markdown transcription did not stabilize")


def transcribe_pages_with_fallbacks(
    client,
    *,
    model: str,
    prompt: str,
    markdown_fallback_prompt: str,
    document_path: Path,
    source_file_bytes: bytes,
    source_mime_type: str,
    primary_file_bytes: bytes,
    primary_mime_type: str,
    preprocess_variant: str,
    generate_fn=None,
    preprocess_fn=preprocess_document_bytes,
    part_factory=None,
) -> tuple[list[dict], str, bytes, str]:
    attempt_variants = ocr_retry_variants(preprocess_variant, source_mime_type)
    last_error: BaseException | None = None

    for _cycle in range(FULL_VARIANT_TRANSCRIPTION_CYCLES):
        for attempt_variant in attempt_variants:
            try:
                if attempt_variant == preprocess_variant:
                    attempt_bytes = primary_file_bytes
                    attempt_mime = primary_mime_type
                else:
                    attempt_bytes, attempt_mime = preprocess_fn(
                        document_path,
                        source_file_bytes,
                        source_mime_type,
                        attempt_variant,
                    )
                pages = attempt_transcription_pages(
                    client,
                    model=model,
                    prompt=prompt,
                    file_bytes=attempt_bytes,
                    mime_type=attempt_mime,
                    generate_fn=generate_fn,
                    part_factory=part_factory,
                )
                return pages, attempt_variant, attempt_bytes, attempt_mime
            except KeyboardInterrupt:
                raise
            except BaseException as exc:
                last_error = exc
                continue
        if last_error is not None and not is_retryable_transcription_error(last_error):
            break

    if last_error is not None and is_retryable_transcription_error(last_error):
        for attempt_variant in attempt_variants:
            try:
                if attempt_variant == preprocess_variant:
                    attempt_bytes = primary_file_bytes
                    attempt_mime = primary_mime_type
                else:
                    attempt_bytes, attempt_mime = preprocess_fn(
                        document_path,
                        source_file_bytes,
                        source_mime_type,
                        attempt_variant,
                    )
                pages = attempt_markdown_transcription_pages(
                    client,
                    model=model,
                    prompt=markdown_fallback_prompt,
                    file_bytes=attempt_bytes,
                    mime_type=attempt_mime,
                    generate_fn=generate_fn,
                    part_factory=part_factory,
                )
                return pages, attempt_variant, attempt_bytes, attempt_mime
            except KeyboardInterrupt:
                raise
            except BaseException as exc:
                last_error = exc
                continue

    if isinstance(last_error, SystemExit):
        raise last_error
    if last_error is not None:
        raise SystemExit(
            f"Gemini transcription failed for all preprocess variants: {last_error}"
        ) from last_error
    raise SystemExit("Gemini transcription failed for all preprocess variants")


def transcribe_document_pages(
    client,
    *,
    model: str,
    document_path: Path,
    source_file_bytes: bytes,
    source_mime_type: str,
    preprocess_variant: str,
    pass_kind: str,
    generate_fn=None,
    preprocess_fn=preprocess_document_bytes,
    localize_fn=maybe_localize_receipt_content,
    transcribe_pages_fn=transcribe_pages_with_fallbacks,
    part_factory=None,
) -> tuple[list[dict], str, bytes, str, bytes, str, bool, dict | None, str]:
    localized_source_file_bytes, localized_source_mime_type, localization_applied, localization_bbox = (
        localize_fn(
            client,
            model=model,
            filename=document_path.name,
            document_path=document_path,
            file_bytes=source_file_bytes,
            mime_type=source_mime_type,
        )
    )

    def transcribe_from_source(
        candidate_source_bytes: bytes,
        candidate_source_mime_type: str,
    ) -> tuple[list[dict], str, bytes, str]:
        prompt = build_prompt(document_path.name, candidate_source_mime_type, pass_kind)
        markdown_fallback_prompt = build_markdown_fallback_prompt(
            document_path.name,
            candidate_source_mime_type,
            pass_kind,
        )
        file_bytes, mime_type = preprocess_fn(
            document_path,
            candidate_source_bytes,
            candidate_source_mime_type,
            preprocess_variant,
        )
        return transcribe_pages_fn(
            client,
            model=model,
            prompt=prompt,
            markdown_fallback_prompt=markdown_fallback_prompt,
            document_path=document_path,
            source_file_bytes=candidate_source_bytes,
            source_mime_type=candidate_source_mime_type,
            primary_file_bytes=file_bytes,
            primary_mime_type=mime_type,
            preprocess_variant=preprocess_variant,
            generate_fn=generate_fn,
            preprocess_fn=preprocess_fn,
            part_factory=part_factory,
        )

    try:
        normalized_pages, effective_preprocess_variant, file_bytes, mime_type = (
            transcribe_from_source(localized_source_file_bytes, localized_source_mime_type)
        )
        return (
            normalized_pages,
            effective_preprocess_variant,
            file_bytes,
            mime_type,
            localized_source_file_bytes,
            localized_source_mime_type,
            localization_applied,
            localization_bbox,
            "gemini_receipt_boundary" if localization_applied else "none",
        )
    except KeyboardInterrupt:
        raise
    except BaseException as exc:
        if not localization_applied or not is_retryable_transcription_error(exc):
            raise

    normalized_pages, effective_preprocess_variant, file_bytes, mime_type = (
        transcribe_from_source(source_file_bytes, source_mime_type)
    )
    return (
        normalized_pages,
        effective_preprocess_variant,
        file_bytes,
        mime_type,
        source_file_bytes,
        source_mime_type,
        False,
        None,
        "fallback_to_original_after_localization",
    )


def attempt_grounding_regions(
    client,
    *,
    model: str,
    prompt: str,
    file_bytes: bytes,
    mime_type: str,
    generate_fn=None,
    part_factory=None,
) -> list[dict]:
    if generate_fn is None:
        generate_fn = generate_content_with_retries
    config = {
        "temperature": 0,
        "response_mime_type": "application/json",
        "max_output_tokens": 4096,
    }
    if part_factory is None:
        from google.genai import types

        part_factory = lambda data, detected_mime: types.Part.from_bytes(  # noqa: E731
            data=data, mime_type=detected_mime
        )
        config = types.GenerateContentConfig(**config)

    response = generate_fn(
        client,
        model=model,
        contents=[
            prompt,
            part_factory(file_bytes, mime_type),
        ],
        config=config,
    )
    payload = json.loads(extract_payload_text(response))
    return normalize_grounding_regions(payload)


def maybe_ground_key_receipt_fields(
    client,
    *,
    model: str,
    filename: str,
    file_bytes: bytes,
    mime_type: str,
    normalized_pages: list[dict],
    document_path: Path | None = None,
    source_file_bytes: bytes | None = None,
    source_mime_type: str | None = None,
    preprocess_variant: str = "original",
    grounding_variants: list[str] | None = None,
    generate_fn=None,
    preprocess_fn=preprocess_document_bytes,
    part_factory=None,
):
    if generate_fn is None:
        generate_fn = generate_content_with_retries
    source_bytes = source_file_bytes if source_file_bytes is not None else file_bytes
    source_mime = source_mime_type or mime_type
    if source_mime not in GROUNDABLE_IMAGE_MIME_TYPES or not normalized_pages:
        return normalized_pages, "none", False, None
    if document_path is None:
        document_path = Path(filename)

    enriched_pages = [dict(page) for page in normalized_pages]
    try:
        dimensions = image_page_dimensions(source_bytes, source_mime)
    except Exception:
        dimensions = None
    if dimensions:
        enriched_pages[0] = dict(enriched_pages[0])
        enriched_pages[0]["dimensions"] = dimensions

    prompt = build_grounding_prompt(filename, str(enriched_pages[0].get("text") or ""))
    attempt_variants = grounding_variants or grounding_retry_variants(preprocess_variant)

    for attempt_variant in attempt_variants:
        try:
            if attempt_variant == preprocess_variant:
                attempt_bytes = file_bytes
                attempt_mime = mime_type
            else:
                attempt_bytes, attempt_mime = preprocess_fn(
                    document_path,
                    source_bytes,
                    source_mime,
                    attempt_variant,
                )
            regions = attempt_grounding_regions(
                client,
                model=model,
                prompt=prompt,
                file_bytes=attempt_bytes,
                mime_type=attempt_mime,
                generate_fn=generate_fn,
                part_factory=part_factory,
            )
        except Exception:
            continue

        if regions:
            enriched_pages[0] = dict(enriched_pages[0])
            enriched_pages[0]["regions"] = regions
            return enriched_pages, "gemini", True, attempt_variant
    return enriched_pages, "none", False, None


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

    source_file_bytes = document_path.read_bytes()
    source_mime_type = detect_mime_type(document_path, source_file_bytes)
    source_file_bytes, source_mime_type = canonicalize_document_bytes_for_ocr(
        document_path,
        source_file_bytes,
        source_mime_type,
    )
    (
        normalized_pages,
        effective_preprocess_variant,
        file_bytes,
        mime_type,
        source_file_bytes,
        source_mime_type,
        localization_applied,
        localization_bbox,
        localization_source,
    ) = transcribe_document_pages(
        client,
        model=args.model,
        document_path=document_path,
        source_file_bytes=source_file_bytes,
        source_mime_type=source_mime_type,
        preprocess_variant=args.preprocess_variant,
        pass_kind=args.pass_kind,
        part_factory=lambda data, detected_mime: types.Part.from_bytes(
            data=data, mime_type=detected_mime
        ),
    )
    try:
        (
            normalized_pages,
            geometry_source,
            geometry_available,
            grounding_preprocess_variant,
        ) = maybe_ground_key_receipt_fields(
            client,
            model=args.model,
            filename=document_path.name,
            file_bytes=file_bytes,
            mime_type=mime_type,
            normalized_pages=normalized_pages,
            document_path=document_path,
            source_file_bytes=source_file_bytes,
            source_mime_type=source_mime_type,
            preprocess_variant=effective_preprocess_variant,
        )
    except Exception:
        geometry_source = "none"
        geometry_available = False
        grounding_preprocess_variant = None

    result = {
        "document_id": sanitize_identifier(document_path.stem),
        "filename": document_path.name,
        "source_path": str(document_path),
        "engine": "vertex_gemini_sdk",
        "metadata": {
            "pass_id": args.pass_id
            or f"{sanitize_identifier(document_path.stem)}_{args.pass_kind}_{effective_preprocess_variant}",
            "pass_kind": args.pass_kind,
            "preprocess_variant": effective_preprocess_variant,
            **(
                {"requested_preprocess_variant": args.preprocess_variant}
                if effective_preprocess_variant != args.preprocess_variant
                else {}
            ),
            **(
                {"grounding_preprocess_variant": grounding_preprocess_variant}
                if grounding_preprocess_variant
                else {}
            ),
            "producer": "google_genai_sdk",
            "model": args.model,
            "geometry_source": geometry_source,
            "geometry_available": geometry_available,
            "localization_applied": localization_applied,
            "localization_source": localization_source,
            **({"localization_bbox": localization_bbox} if localization_bbox else {}),
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
