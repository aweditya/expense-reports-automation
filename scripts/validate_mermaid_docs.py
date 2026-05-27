#!/usr/bin/env python3
"""Extract every fenced mermaid block from docs/*.md and README.md.
Render each in headless Chromium using mermaid.js from CDN.
Fail on parse errors. Exits non-zero if any diagram fails.

Usage:
    ./.venv/bin/python scripts/validate_mermaid_docs.py
    ./.venv/bin/python scripts/validate_mermaid_docs.py --save-svgs

Without --save-svgs, validation runs in memory and prints a summary.
With --save-svgs, rendered SVGs are written under .scratch/mermaid-renders/
for manual eyeball.

Requires Playwright + Chromium installed:
    ./.venv/bin/pip install playwright
    ./.venv/bin/playwright install chromium
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DOCS_DIR = REPO_ROOT / "docs"
README = REPO_ROOT / "README.md"
SVG_OUT_DIR = REPO_ROOT / ".scratch" / "mermaid-renders"

FENCE_RE = re.compile(r"```mermaid\n(.*?)\n```", re.DOTALL)

HTML_HARNESS = """
<!DOCTYPE html>
<html><head>
<script type="module">
  import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.esm.min.mjs';
  mermaid.initialize({ startOnLoad: false, theme: 'default' });
  window.renderMermaid = async function(src) {
    try {
      const { svg } = await mermaid.render('d', src);
      return { ok: true, svg };
    } catch (err) {
      return { ok: false, error: String(err && (err.message || err)) };
    }
  };
  window.MERMAID_READY = true;
</script>
</head><body></body></html>
"""


def extract_blocks(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    return FENCE_RE.findall(text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--save-svgs", action="store_true",
                        help="write rendered SVGs under .scratch/mermaid-renders/")
    args = parser.parse_args()

    sources: list[tuple[Path, int, str]] = []
    for path in sorted(DOCS_DIR.glob("*.md")):
        for idx, block in enumerate(extract_blocks(path)):
            sources.append((path, idx, block))
    if README.exists():
        for idx, block in enumerate(extract_blocks(README)):
            sources.append((README, idx, block))

    if not sources:
        print("no mermaid blocks found")
        return 0

    print(f"found {len(sources)} mermaid block(s) across "
          f"{len({s[0] for s in sources})} file(s)")

    from playwright.sync_api import sync_playwright

    failures: list[tuple[Path, int, str]] = []
    if args.save_svgs:
        SVG_OUT_DIR.mkdir(parents=True, exist_ok=True)

    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        ctx = browser.new_context()
        page = ctx.new_page()
        page.set_content(HTML_HARNESS, wait_until="domcontentloaded")
        page.wait_for_function("() => window.MERMAID_READY === true",
                               timeout=15_000)

        for path, idx, src in sources:
            result = page.evaluate("(src) => window.renderMermaid(src)", src)
            label = f"{path.name}#{idx}"
            if result.get("ok"):
                print(f"  ok    {label}")
                if args.save_svgs:
                    out = SVG_OUT_DIR / f"{path.stem}-{idx}.svg"
                    out.write_text(result["svg"], encoding="utf-8")
            else:
                err = result.get("error", "(no error message)")
                print(f"  FAIL  {label}: {err}")
                failures.append((path, idx, err))

        ctx.close()
        browser.close()

    print()
    if failures:
        print(f"FAILED: {len(failures)} diagram(s)")
        for path, idx, err in failures:
            print(f"  {path.name}#{idx}: {err}")
        return 1
    print(f"OK: {len(sources)} diagram(s) rendered cleanly")
    return 0


if __name__ == "__main__":
    sys.exit(main())
