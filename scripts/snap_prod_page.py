#!/usr/bin/env python3
"""One-off Playwright screenshotter for an authed prod URL.

Usage:
    ./.venv/bin/python scripts/snap_prod_page.py <path> <out.png>
    ./.venv/bin/python scripts/snap_prod_page.py /          .scratch/dashboard.png

Avoids inline-script shortcuts (regrets log: 7+ slips on python -c
imports). Drop a real script in scripts/ instead.
"""
import subprocess
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright

PROD_URL = "https://expense-reports-wgnivgelea-uw.a.run.app"


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    path, out = sys.argv[1], Path(sys.argv[2])
    out.parent.mkdir(parents=True, exist_ok=True)
    token = subprocess.run(["gcloud", "auth", "print-identity-token"],
                           capture_output=True, check=True, timeout=10
                           ).stdout.decode().strip()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        ctx = browser.new_context(
            viewport={"width": 1400, "height": 1400},
            extra_http_headers={"Authorization": f"Bearer {token}"},
        )
        page = ctx.new_page()
        page.goto(PROD_URL + path, wait_until="networkidle", timeout=30_000)
        page.screenshot(path=str(out), full_page=True)
        ctx.close()
        browser.close()
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
