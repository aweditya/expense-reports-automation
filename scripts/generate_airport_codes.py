#!/usr/bin/env python3
"""One-shot codegen: extract Stanford's airport-name lookup table from
the FA-supplied foreign-template xlsx into `generated/airport_codes.json`.

Stanford's foreign-portal expense template expects airport cells in
the format `IATA - Full Name (City, …)` — e.g. `SFO - San Francisco
International (San Francisco, CA)`. Raw IATA codes (`SFO`) get
rejected by the upload validator. The xlsx ships two sheets that are
identical (`Departure Airport`, `Destination Airport`), each ~9208
entries. This script parses one sheet and emits a `{iata_code:
full_display_string}` JSON map that the Rust side embeds via
`include_str!` and looks up at render time.

Source: `reference/ers-template-foreign-example-filled.xlsx`,
sheet `Departure Airport` (chosen arbitrarily over `Destination
Airport` — same content).

Run after Stanford updates the template; otherwise the generated
file is stable. Idempotent.
"""

from __future__ import annotations

import json
import re
import sys
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
XLSX = REPO_ROOT / "reference" / "ers-template-foreign-example-filled.xlsx"
OUT = REPO_ROOT / "generated" / "airport_codes.json"

NS = {"main": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}

# IATA codes are 3 uppercase letters; some are 4 alphanumeric (e.g. small
# airports). Stanford's sheet uses the leading-token-before-` - ` as the
# code. Allow 3-4 chars to be safe; reject anything that doesn't match.
IATA_RE = re.compile(r"^([A-Z0-9]{2,4})\s+-\s+")


def load_shared_strings(z: zipfile.ZipFile) -> list[str]:
    xml = z.read("xl/sharedStrings.xml").decode("utf-8")
    root = ET.fromstring(xml)
    out: list[str] = []
    for si in root.findall("main:si", NS):
        # Concat all <t> children to handle rich-text entries.
        text = "".join(
            t.text or "" for t in si.iter("{http://schemas.openxmlformats.org/spreadsheetml/2006/main}t")
        )
        out.append(text)
    return out


def sheet_index_by_name(z: zipfile.ZipFile, target_name: str) -> int | None:
    wb = ET.fromstring(z.read("xl/workbook.xml").decode("utf-8"))
    for i, sh in enumerate(
        wb.iter("{http://schemas.openxmlformats.org/spreadsheetml/2006/main}sheet"), 1
    ):
        if sh.attrib.get("name") == target_name:
            return i
    return None


def read_column_a(z: zipfile.ZipFile, sheet_idx: int, strings: list[str]) -> list[str]:
    sx = ET.fromstring(z.read(f"xl/worksheets/sheet{sheet_idx}.xml").decode("utf-8"))
    out: list[str] = []
    for row in sx.iter("{http://schemas.openxmlformats.org/spreadsheetml/2006/main}row"):
        for c in row.findall("main:c", NS):
            ref = c.attrib["r"]
            # column letters are everything up to the first digit
            col = re.match(r"^[A-Z]+", ref).group()
            if col != "A":
                continue
            t = c.attrib.get("t", "n")
            v_el = c.find("main:v", NS)
            v = v_el.text if v_el is not None else ""
            if t == "s" and v:
                v = strings[int(v)]
            if v:
                out.append(v)
    return out


def parse_entries(rows: list[str]) -> dict[str, str]:
    """Pull the IATA code out of each entry. Skip rows that don't match
    the canonical `IATA - Full Name (...)` shape so we don't store
    garbage. Duplicates (same code, different display) keep the first
    occurrence — Stanford's sheet has occasional duplicates and the
    first wins by convention."""
    out: dict[str, str] = {}
    skipped: list[str] = []
    for row in rows:
        m = IATA_RE.match(row)
        if not m:
            skipped.append(row)
            continue
        code = m.group(1)
        if code in out:
            continue  # keep first
        out[code] = row
    if skipped:
        print(f"# skipped {len(skipped)} rows that didn't match IATA pattern", file=sys.stderr)
        for s in skipped[:5]:
            print(f"    {s!r}", file=sys.stderr)
    return out


def main() -> int:
    if not XLSX.exists():
        print(f"error: {XLSX} not found", file=sys.stderr)
        return 2

    with zipfile.ZipFile(XLSX) as z:
        strings = load_shared_strings(z)
        sheet_idx = sheet_index_by_name(z, "Departure Airport")
        if sheet_idx is None:
            print("error: no 'Departure Airport' sheet found", file=sys.stderr)
            return 2
        rows = read_column_a(z, sheet_idx, strings)

    entries = parse_entries(rows)
    print(f"# parsed {len(entries)} airport codes from {len(rows)} rows", file=sys.stderr)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(entries, indent=2, ensure_ascii=False, sort_keys=True))
    print(f"# wrote {OUT}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
