"""Spike: dump an .xlsx/.xlsm sheet as plain cell triples.

Usage:  ./.venv/bin/python scripts/spike_dump_xlsx.py <file.xlsx> [sheet_index]

Stdlib only — uses zipfile + xml.etree, no openpyxl dep. Lets me read
the FA-shared ERS Template without adding a runtime dep just for an
exploratory pass. If we end up generating .xlsx programmatically in
production, openpyxl belongs in deploy/requirements.txt — but right
now we just want to *see* the columns the FA expects.
"""

from __future__ import annotations

import sys
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

NS = {"x": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}


def col_letter(idx: int) -> str:
    s = ""
    while idx >= 0:
        s = chr(ord("A") + idx % 26) + s
        idx = idx // 26 - 1
    return s


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: spike_dump_xlsx.py <file.xlsx> [sheet_index]", file=sys.stderr)
        return 2
    path = Path(sys.argv[1])
    sheet_idx = int(sys.argv[2]) if len(sys.argv) > 2 else 1

    with zipfile.ZipFile(path) as z:
        # Shared strings: index → text
        strings: list[str] = []
        if "xl/sharedStrings.xml" in z.namelist():
            ss_root = ET.fromstring(z.read("xl/sharedStrings.xml"))
            for si in ss_root.findall("x:si", NS):
                texts = [t.text or "" for t in si.findall(".//x:t", NS)]
                strings.append("".join(texts))

        # Workbook → sheet names
        wb_root = ET.fromstring(z.read("xl/workbook.xml"))
        sheets = [(s.get("name"), s.get("sheetId")) for s in wb_root.findall(".//x:sheet", NS)]
        name = sheets[sheet_idx - 1][0] if sheets else f"sheet{sheet_idx}"
        print(f"# sheet {sheet_idx}: {name!r}")
        print(f"# total sheets: {[s[0] for s in sheets]}")
        print()

        sheet_xml = z.read(f"xl/worksheets/sheet{sheet_idx}.xml")
        ws_root = ET.fromstring(sheet_xml)
        for row in ws_root.findall(".//x:row", NS):
            r = row.get("r")
            for c in row.findall("x:c", NS):
                ref = c.get("r")
                t = c.get("t")
                v = c.find("x:v", NS)
                if v is None or v.text is None:
                    continue
                raw = v.text
                if t == "s":
                    val = strings[int(raw)] if int(raw) < len(strings) else f"<#{raw}>"
                else:
                    val = raw
                # Truncate long values
                if len(val) > 80:
                    val = val[:77] + "..."
                print(f"  {ref:<6}  {val!r}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
