#!/usr/bin/env python3

import argparse
import difflib
import json
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare source markdown against a transcribed-document JSON artifact."
    )
    parser.add_argument("source_markdown", help="Original source markdown file")
    parser.add_argument("transcribed_json", help="Transcribed-document JSON artifact")
    return parser.parse_args()


def normalize_markdown(text: str) -> str:
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").split("\n")]
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def relax_markdown(text: str) -> str:
    relaxed_lines = []
    previous_blank = False
    for line in normalize_markdown(text).split("\n"):
        is_blank = not line
        if is_blank and previous_blank:
            continue
        relaxed_lines.append(line)
        previous_blank = is_blank
    return "\n".join(relaxed_lines)


def content_only_markdown(text: str) -> str:
    return "\n".join(
        line for line in normalize_markdown(text).split("\n") if line.strip()
    )


def join_transcribed_pages(payload: dict) -> str:
    pages = payload.get("pages") or []
    return "\n\n".join(page["text"] for page in pages)


def main() -> int:
    args = parse_args()
    source_text = normalize_markdown(Path(args.source_markdown).read_text())
    transcribed_payload = json.loads(Path(args.transcribed_json).read_text())
    transcribed_text = normalize_markdown(join_transcribed_pages(transcribed_payload))

    exact_match = source_text == transcribed_text
    relaxed_match = relax_markdown(source_text) == relax_markdown(transcribed_text)
    content_match = content_only_markdown(source_text) == content_only_markdown(
        transcribed_text
    )
    print(f"exact_match={str(exact_match).lower()}")
    print(f"relaxed_match={str(relaxed_match).lower()}")
    print(f"content_match={str(content_match).lower()}")

    if exact_match:
        return 0

    diff = difflib.unified_diff(
        source_text.splitlines(),
        transcribed_text.splitlines(),
        fromfile=args.source_markdown,
        tofile=args.transcribed_json,
        lineterm="",
    )
    for line in diff:
        print(line)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
