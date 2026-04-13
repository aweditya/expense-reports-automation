#!/usr/bin/env python3

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


DEFAULT_PAGE_WIDTH = 1700
DEFAULT_PAGE_HEIGHT = 2200
DEFAULT_MARGIN = 120
DEFAULT_FONT_SIZE = 30
DEFAULT_LINE_SPACING = 14


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render plain-text or markdown documents into OCR-style PNG/PDF fixtures."
    )
    parser.add_argument("inputs", nargs="+", help="Input markdown/text files to render")
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where rendered OCR fixtures should be written",
    )
    parser.add_argument(
        "--format",
        choices=("png", "pdf", "both"),
        default="both",
        help="Output format to emit",
    )
    parser.add_argument(
        "--page-width", type=int, default=DEFAULT_PAGE_WIDTH, help="Rendered page width in pixels"
    )
    parser.add_argument(
        "--page-height",
        type=int,
        default=DEFAULT_PAGE_HEIGHT,
        help="Rendered page height in pixels",
    )
    parser.add_argument(
        "--margin", type=int, default=DEFAULT_MARGIN, help="Page margin in pixels"
    )
    parser.add_argument(
        "--font-size", type=int, default=DEFAULT_FONT_SIZE, help="Monospace font size in pixels"
    )
    parser.add_argument(
        "--line-spacing",
        type=int,
        default=DEFAULT_LINE_SPACING,
        help="Extra spacing added between rendered lines",
    )
    return parser.parse_args()


def load_font(font_size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    candidates = [
        "/System/Library/Fonts/Supplemental/Courier New.ttf",
        "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
        "/System/Library/Fonts/Monaco.ttf",
    ]
    for candidate in candidates:
        path = Path(candidate)
        if path.exists():
            return ImageFont.truetype(str(path), font_size)
    return ImageFont.load_default()


def measure_text(draw: ImageDraw.ImageDraw, font, text: str) -> int:
    if not text:
        text = " "
    left, _, right, _ = draw.textbbox((0, 0), text, font=font)
    return right - left


def wrap_line(draw: ImageDraw.ImageDraw, font, text: str, max_width: int) -> list[str]:
    if not text:
        return [""]

    if measure_text(draw, font, text) <= max_width:
        return [text]

    prefix = ""
    remainder = text
    for marker in ("- ", "* ", "  - ", "  * ", "## ", "# "):
        if remainder.startswith(marker):
            prefix = marker
            remainder = remainder[len(marker) :]
            break

    words = remainder.split()
    if not words:
        return [text]

    wrapped: list[str] = []
    current = prefix.rstrip()
    first_line = True
    for word in words:
        candidate_prefix = prefix if first_line else " " * len(prefix)
        candidate = f"{current} {word}".strip() if current.strip() else f"{candidate_prefix}{word}"
        if measure_text(draw, font, candidate) <= max_width:
            current = candidate
            continue

        if current:
            wrapped.append(current)
        current = f"{candidate_prefix}{word}"
        first_line = False

    if current:
        wrapped.append(current)
    return wrapped


def paginate_lines(
    lines: list[str],
    font,
    page_width: int,
    page_height: int,
    margin: int,
    line_spacing: int,
) -> list[Image.Image]:
    probe = Image.new("RGB", (page_width, page_height), "white")
    probe_draw = ImageDraw.Draw(probe)
    _, _, _, bottom = probe_draw.textbbox((0, 0), "Ag", font=font)
    line_height = bottom + line_spacing
    max_width = page_width - margin * 2
    max_y = page_height - margin

    pages: list[Image.Image] = []
    image = Image.new("RGB", (page_width, page_height), "white")
    draw = ImageDraw.Draw(image)
    y = margin

    def flush_page() -> None:
        nonlocal image, draw, y
        pages.append(image)
        image = Image.new("RGB", (page_width, page_height), "white")
        draw = ImageDraw.Draw(image)
        y = margin

    for line in lines:
        for wrapped_line in wrap_line(draw, font, line, max_width):
            if y + line_height > max_y:
                flush_page()
            draw.text((margin, y), wrapped_line, fill="black", font=font)
            y += line_height

    pages.append(image)
    return pages


def render_document(
    input_path: Path,
    output_dir: Path,
    output_format: str,
    page_width: int,
    page_height: int,
    margin: int,
    font,
    line_spacing: int,
) -> list[Path]:
    text = input_path.read_text()
    lines = text.splitlines()
    pages = paginate_lines(lines, font, page_width, page_height, margin, line_spacing)
    stem = input_path.stem
    output_paths: list[Path] = []

    if output_format in {"png", "both"}:
        for page_number, page in enumerate(pages, start=1):
            if len(pages) == 1:
                png_path = output_dir / f"{stem}.png"
            else:
                png_path = output_dir / f"{stem}_page_{page_number}.png"
            page.save(png_path)
            output_paths.append(png_path)

    if output_format in {"pdf", "both"}:
        pdf_path = output_dir / f"{stem}.pdf"
        pdf_pages = [page.convert("RGB") for page in pages]
        pdf_pages[0].save(pdf_path, save_all=True, append_images=pdf_pages[1:])
        output_paths.append(pdf_path)

    return output_paths


def main() -> int:
    args = parse_args()
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    font = load_font(args.font_size)

    for input_name in args.inputs:
        input_path = Path(input_name)
        if not input_path.exists():
            raise SystemExit(f"input file does not exist: {input_path}")
        render_document(
            input_path=input_path,
            output_dir=output_dir,
            output_format=args.format,
            page_width=args.page_width,
            page_height=args.page_height,
            margin=args.margin,
            font=font,
            line_spacing=args.line_spacing,
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
