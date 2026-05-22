#!/usr/bin/env python3
"""Subscribe to /upload/progress/<id> and print each phase update until
the pipeline reaches a terminal state (done/error/not_found/lost).

Used by smoke tests + eyeballs that need to block until a pipeline
completes. Not an inline script — addresses the regret about embedding
`python -c "…"` in ad-hoc bash chains during eyeballs.

Usage:
  scripts/sse_wait_for_phase.py <base_url> <upload_id>
"""

from __future__ import annotations

import json
import sys
import urllib.request

TERMINAL = {"done", "error", "not_found", "lost"}


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: sse_wait_for_phase.py <base_url> <upload_id>", file=sys.stderr)
        return 2
    base_url, upload_id = sys.argv[1], sys.argv[2]
    url = f"{base_url}/upload/progress/{upload_id}"
    with urllib.request.urlopen(url, timeout=300) as r:
        for raw in r:
            line = raw.decode("utf-8", "replace").strip()
            if not line.startswith("data:"):
                continue
            try:
                snap = json.loads(line[len("data:"):].strip())
            except json.JSONDecodeError:
                continue
            files = snap.get("files", [])
            statuses = [
                (f.get("name", "?")[:32], f.get("status"), (f.get("error") or "")[:80])
                for f in files
            ]
            print(f"phase={snap.get('phase')}  files={statuses}")
            if snap.get("phase") in TERMINAL:
                return 0 if snap.get("phase") == "done" else 1
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
