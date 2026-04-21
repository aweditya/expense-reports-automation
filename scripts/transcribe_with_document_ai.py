#!/usr/bin/env python3

import argparse
import io
import json
from pathlib import Path


CLOUD_PLATFORM_SCOPE = "https://www.googleapis.com/auth/cloud-platform"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Transcribe a PDF or image document into typed OCR JSON using Document AI."
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
        default="us",
        help="Document AI location, for example us or eu",
    )
    parser.add_argument(
        "--processor-id",
        help="Optional OCR processor id. If omitted, the script auto-selects the first OCR processor in the project/location.",
    )
    parser.add_argument(
        "--processor-version",
        help="Optional processor version id",
    )
    parser.add_argument("--pass-id", help="Stable OCR pass identifier for emitted metadata")
    parser.add_argument(
        "--pass-kind",
        default="primary",
        help="OCR pass kind metadata, for example primary or geometry_assist",
    )
    parser.add_argument(
        "--preprocess-variant",
        default="original",
        help="Preprocessing variant metadata, for example original or contrast_boosted",
    )
    parser.add_argument("--output", help="Optional output path for the rendered JSON artifact")
    return parser.parse_args()


def load_project_id(service_account_key: Path, explicit_project: str | None) -> str:
    if explicit_project:
        return explicit_project
    payload = json.loads(service_account_key.read_text())
    project_id = payload.get("project_id")
    if not project_id:
        raise SystemExit("service-account key did not include project_id; pass --project")
    return project_id


def detect_mime_type(path: Path) -> str:
    extension = path.suffix.lower()
    if extension == ".pdf":
        return "application/pdf"
    if extension == ".png":
        return "image/png"
    if extension in {".jpg", ".jpeg"}:
        return "image/jpeg"
    raise SystemExit(f"unsupported document format: {path.suffix}")


def preprocess_document_bytes(
    file_bytes: bytes, mime_type: str, preprocess_variant: str
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


def build_client(credentials, location: str):
    from google.api_core.client_options import ClientOptions
    from google.cloud import documentai

    endpoint = f"{location}-documentai.googleapis.com"
    return documentai.DocumentProcessorServiceClient(
        credentials=credentials,
        client_options=ClientOptions(api_endpoint=endpoint),
    )


def resolve_processor_name(
    client,
    *,
    project_id: str,
    location: str,
    processor_id: str | None,
    processor_version: str | None,
) -> str:
    if processor_id:
        if processor_version:
            return client.processor_version_path(
                project_id, location, processor_id, processor_version
            )
        return client.processor_path(project_id, location, processor_id)

    parent = client.common_location_path(project_id, location)
    candidates = []
    for processor in client.list_processors(request={"parent": parent}):
        processor_type = str(getattr(processor, "type_", "") or "").upper()
        if processor_type == "OCR_PROCESSOR" or "OCR" in processor_type:
            candidates.append(processor)
    if not candidates:
        raise SystemExit(
            "no OCR processor was found in the configured project/location; pass --processor-id or create an OCR processor first"
        )
    candidates.sort(key=lambda processor: getattr(processor, "name", ""))
    return candidates[0].name


def extract_text_from_anchor(document_text: str, text_anchor) -> str:
    if text_anchor is None:
        return ""
    text_segments = getattr(text_anchor, "text_segments", None) or []
    if not text_segments:
        return ""
    parts = []
    for segment in text_segments:
        start_index = int(getattr(segment, "start_index", 0) or 0)
        end_index = getattr(segment, "end_index", None)
        end_index = len(document_text) if end_index in {None, ""} else int(end_index)
        if end_index <= start_index:
            continue
        parts.append(document_text[start_index:end_index])
    return "".join(parts).strip()


def page_dimensions(page) -> dict | None:
    dimension = getattr(page, "dimension", None)
    if dimension is None:
        return None
    width = getattr(dimension, "width", None)
    height = getattr(dimension, "height", None)
    if width is None or height is None:
        return None
    return {"width": max(0, round(float(width))), "height": max(0, round(float(height)))}


def layout_bbox(layout, page) -> dict | None:
    bounding_poly = getattr(layout, "bounding_poly", None)
    if bounding_poly is None:
        return None

    normalized_vertices = getattr(bounding_poly, "normalized_vertices", None) or []
    if normalized_vertices:
        xs = [float(vertex.x) for vertex in normalized_vertices]
        ys = [float(vertex.y) for vertex in normalized_vertices]
        return bbox_from_points(xs, ys)

    vertices = getattr(bounding_poly, "vertices", None) or []
    if not vertices:
        return None

    dimensions = page_dimensions(page)
    if not dimensions or not dimensions["width"] or not dimensions["height"]:
        return None
    xs = [float(vertex.x) / float(dimensions["width"]) for vertex in vertices]
    ys = [float(vertex.y) / float(dimensions["height"]) for vertex in vertices]
    return bbox_from_points(xs, ys)


def bbox_from_points(xs: list[float], ys: list[float]) -> dict | None:
    if not xs or not ys:
        return None
    left = min(xs)
    top = min(ys)
    right = max(xs)
    bottom = max(ys)
    if right <= left or bottom <= top:
        return None
    return {
        "left": max(0.0, left),
        "top": max(0.0, top),
        "width": min(1.0, right) - max(0.0, left),
        "height": min(1.0, bottom) - max(0.0, top),
    }


def append_layout_regions(regions: list[dict], *, page, items, kind: str, prefix: str, document_text: str):
    for index, item in enumerate(items, start=1):
        layout = getattr(item, "layout", None)
        if layout is None:
            continue
        text = extract_text_from_anchor(document_text, getattr(layout, "text_anchor", None))
        if not text:
            continue
        regions.append(
            {
                "region_id": f"{prefix}_{index}",
                "kind": kind,
                "text": " ".join(text.split()),
                "bbox": layout_bbox(layout, page),
            }
        )


def page_regions(page, document_text: str) -> list[dict]:
    regions = []
    append_layout_regions(
        regions,
        page=page,
        items=getattr(page, "blocks", None) or [],
        kind="block",
        prefix=f"page_{getattr(page, 'page_number', 1)}_block",
        document_text=document_text,
    )
    append_layout_regions(
        regions,
        page=page,
        items=getattr(page, "lines", None) or [],
        kind="line",
        prefix=f"page_{getattr(page, 'page_number', 1)}_line",
        document_text=document_text,
    )
    append_layout_regions(
        regions,
        page=page,
        items=getattr(page, "tokens", None) or [],
        kind="token",
        prefix=f"page_{getattr(page, 'page_number', 1)}_token",
        document_text=document_text,
    )
    for table_index, table in enumerate(getattr(page, "tables", None) or [], start=1):
        layout = getattr(table, "layout", None)
        if layout is not None:
            text = extract_text_from_anchor(document_text, getattr(layout, "text_anchor", None))
            if text:
                regions.append(
                    {
                        "region_id": f"page_{getattr(page, 'page_number', 1)}_table_{table_index}",
                        "kind": "table",
                        "text": " ".join(text.split()),
                        "bbox": layout_bbox(layout, page),
                    }
                )
        for row_index, row in enumerate(getattr(table, "body_rows", None) or [], start=1):
            for cell_index, cell in enumerate(getattr(row, "cells", None) or [], start=1):
                layout = getattr(cell, "layout", None)
                if layout is None:
                    continue
                text = extract_text_from_anchor(document_text, getattr(layout, "text_anchor", None))
                if not text:
                    continue
                regions.append(
                    {
                        "region_id": f"page_{getattr(page, 'page_number', 1)}_table_{table_index}_cell_{row_index}_{cell_index}",
                        "kind": "table_cell",
                        "text": " ".join(text.split()),
                        "bbox": layout_bbox(layout, page),
                    }
                )
    return regions


def page_text(page, document_text: str) -> str:
    layout = getattr(page, "layout", None)
    if layout is not None:
        text = extract_text_from_anchor(document_text, getattr(layout, "text_anchor", None))
        if text:
            return text.strip()
    line_texts = []
    for line in getattr(page, "lines", None) or []:
        layout = getattr(line, "layout", None)
        if layout is None:
            continue
        text = extract_text_from_anchor(document_text, getattr(layout, "text_anchor", None))
        if text:
            line_texts.append(" ".join(text.split()))
    return "\n".join(line_texts).strip()


def normalize_document(document, *, filename: str, source_path: Path, pass_id: str, pass_kind: str, preprocess_variant: str, processor_name: str) -> dict:
    document_text = getattr(document, "text", "") or ""
    pages = []
    geometry_available = False

    for page_index, page in enumerate(getattr(document, "pages", None) or [], start=1):
        text = page_text(page, document_text)
        regions = page_regions(page, document_text)
        geometry_available = geometry_available or any(region.get("bbox") for region in regions)
        pages.append(
            {
                "page_number": getattr(page, "page_number", None) or page_index,
                "text": text,
                "dimensions": page_dimensions(page),
                "regions": regions,
            }
        )

    if not pages:
        raise SystemExit("Document AI response did not contain any pages")

    return {
        "document_id": sanitize_identifier(source_path.stem),
        "filename": filename,
        "source_path": str(source_path),
        "metadata": {
            "pass_id": pass_id,
            "pass_kind": pass_kind,
            "preprocess_variant": preprocess_variant,
            "producer": "document_ai_sdk",
            "model": processor_name,
            "geometry_source": "document_ai",
            "geometry_available": geometry_available,
        },
        "pages": pages,
    }


def main() -> None:
    args = parse_args()
    document_path = Path(args.document).resolve()
    service_account_key = Path(args.service_account_key).resolve()
    project_id = load_project_id(service_account_key, args.project)
    mime_type = detect_mime_type(document_path)
    file_bytes = document_path.read_bytes()
    processed_bytes, processed_mime_type = preprocess_document_bytes(
        file_bytes, mime_type, args.preprocess_variant
    )
    pass_id = args.pass_id or f"document_ai_{args.pass_kind}_{args.preprocess_variant}"

    from google.cloud import documentai
    from google.oauth2 import service_account

    credentials = service_account.Credentials.from_service_account_file(
        service_account_key,
        scopes=[CLOUD_PLATFORM_SCOPE],
    )
    client = build_client(credentials, args.location)
    processor_name = resolve_processor_name(
        client,
        project_id=project_id,
        location=args.location,
        processor_id=args.processor_id,
        processor_version=args.processor_version,
    )

    raw_document = documentai.RawDocument(
        content=processed_bytes,
        mime_type=processed_mime_type,
    )
    request = documentai.ProcessRequest(name=processor_name, raw_document=raw_document)
    result = client.process_document(request=request)
    payload = normalize_document(
        result.document,
        filename=document_path.name,
        source_path=document_path,
        pass_id=pass_id,
        pass_kind=args.pass_kind,
        preprocess_variant=args.preprocess_variant,
        processor_name=processor_name,
    )

    rendered = json.dumps(payload, indent=2)
    if args.output:
        Path(args.output).write_text(rendered)
    else:
        print(rendered)


if __name__ == "__main__":
    main()
