"""Verification spike for the HEIC content-sniff preflight in local_app_simple.

Exercises three cases against the new helpers:

1. HEIC bytes (synthesized in-process via Pillow + pillow-heif so we don't
   need a checked-in HEIC fixture) — must be detected and transcoded to JPEG;
   filename must gain the .jpg extension.
2. Real JPEG bytes (the tamarine receipt, which is now an actual JPEG after
   the 2026-05-18 corpus cleanup) — must pass through unchanged.
3. Synthesized PNG bytes — must pass through unchanged.

Also asserts the failure path: hand the converter HEIC-shaped bytes that
aren't actually decodable, confirm it logs and returns None instead of
raising.

Writes any synthesized fixtures under .scratch/heic_spike/ (per CLAUDE.md
rule 5: never /tmp).
"""

from __future__ import annotations

import io
import sys
from pathlib import Path

from PIL import Image
from pillow_heif import register_heif_opener

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from local_app_simple import (  # noqa: E402
    convert_heic_to_jpeg_if_needed,
    is_heic_bytes,
)

register_heif_opener()

SCRATCH = REPO_ROOT / ".scratch" / "heic_spike"
SCRATCH.mkdir(parents=True, exist_ok=True)


def synth_heic_bytes() -> bytes:
    """Render a tiny solid-color image and encode it as HEIC."""
    img = Image.new("RGB", (64, 48), color=(200, 80, 80))
    buf = io.BytesIO()
    img.save(buf, format="HEIF")
    return buf.getvalue()


def synth_png_bytes() -> bytes:
    img = Image.new("RGB", (32, 24), color=(80, 200, 80))
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def case_heic_detected_and_transcoded() -> None:
    heic = synth_heic_bytes()
    (SCRATCH / "synth.heic").write_bytes(heic)
    assert is_heic_bytes(heic), "synthesized HEIC bytes should sniff as HEIC"

    # Caller may pass a misleading .png extension (the whole point of this
    # feature — iOS-exported HEIC bytes often arrive with a .png name).
    result = convert_heic_to_jpeg_if_needed(heic, "iphone_photo.png")
    assert result is not None, "HEIC bytes must be transcoded, not passed through"
    jpeg_bytes, new_name = result
    assert new_name == "iphone_photo.jpg", f"unexpected new name: {new_name!r}"
    # Smoke: the returned bytes must round-trip through Pillow as JPEG.
    with Image.open(io.BytesIO(jpeg_bytes)) as img:
        assert img.format == "JPEG", f"expected JPEG, got {img.format}"
        assert img.size == (64, 48), f"size lost in transcode: {img.size}"
    (SCRATCH / "synth_transcoded.jpg").write_bytes(jpeg_bytes)
    print(
        f"  HEIC case OK: {len(heic)} bytes HEIC → {len(jpeg_bytes)} bytes JPEG "
        f"(iphone_photo.png → {new_name})"
    )


def case_real_jpeg_passthrough() -> None:
    tamarine = REPO_ROOT / "receipts" / "meal_2026-03-05_tamarine-palo-alto.jpg"
    if not tamarine.exists():
        print(f"  SKIP real-JPEG case: {tamarine} not present")
        return
    data = tamarine.read_bytes()
    assert not is_heic_bytes(data), "real JPEG must not sniff as HEIC"
    result = convert_heic_to_jpeg_if_needed(data, tamarine.name)
    assert result is None, "real JPEG should pass through (got transcode result)"
    print(f"  JPEG passthrough OK: {tamarine.name} ({len(data)} bytes) untouched")


def case_png_passthrough() -> None:
    png = synth_png_bytes()
    assert not is_heic_bytes(png), "PNG must not sniff as HEIC"
    result = convert_heic_to_jpeg_if_needed(png, "logo.png")
    assert result is None, "PNG should pass through (got transcode result)"
    print(f"  PNG passthrough OK: {len(png)} bytes untouched")


def case_corrupt_heic_falls_back() -> None:
    # Looks like HEIC at the magic-number level (ftyp + heic brand) but the
    # rest of the buffer is garbage; Pillow should raise on decode and our
    # helper should swallow it and return None.
    fake = b"\x00\x00\x00\x18ftypheic" + b"\x00" * 64
    assert is_heic_bytes(fake), "test fixture must sniff as HEIC"
    result = convert_heic_to_jpeg_if_needed(fake, "broken.heic")
    assert result is None, "corrupt HEIC must fall back to original bytes"
    print("  corrupt-HEIC fallback OK: returned None (caller keeps original)")


def main() -> int:
    print("spike_heic_preflight: exercising HEIC content-sniff + transcode")
    case_heic_detected_and_transcoded()
    case_real_jpeg_passthrough()
    case_png_passthrough()
    case_corrupt_heic_falls_back()
    print("all cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
