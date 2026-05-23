"""Test 6+ receipts in a single upload against the deployed Cloud Run
service. Validates Stage 11c per-file isolation on prod + measures
real multi-receipt wallclock so we know what FAs are seeing.

Uses gcloud's identity token + the direct Cloud Run URL to bypass
IAP (the deploy-cheatsheet's documented test path — see "Verify the
deployed app from the CLI").

Cost: ~$0.10 per receipt × N receipts in Gemini calls.
Time: depends on N and Gemini throughput (~30-60s per receipt
sequential at the moment).

Usage:
  scripts/test_prod_multi_receipt.py                    # 6 default receipts
  scripts/test_prod_multi_receipt.py --base-url http://127.0.0.1:8765
                                                          # against local Flask
  scripts/test_prod_multi_receipt.py --receipt path1 \\
                                     --receipt path2 ...  # custom files
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RECEIPTS_DIR = REPO_ROOT / "receipts"
DEFAULT_BASE_URL = "https://expense-reports-wgnivgelea-uw.a.run.app"

# Default receipt pick: 1 airfare + 2 lodging + 1 transport + 2 meal
# (where meal/transport are PDFs that the extractors handle well).
DEFAULT_RECEIPTS: list[tuple[str, str]] = [
    ("airfare", "airfare_2026-03-21_egencia-united-sfo-pit-roundtrip.pdf"),
    ("lodging", "lodging_2023-01-09_warren-hotel-v2.pdf"),
    ("lodging", "lodging_2023-04-16_apa-hotel-kanda-via-agoda.pdf"),
    ("transport", "transport_2023-01-05_lyft-las-vegas-strip-to-paradise.pdf"),
    ("transport", "transport_2023-01-08_lyft-dtw-to-novi.pdf"),
    ("meal", "meal_2026-03-22_pittsburgh-dinner.pdf"),
]


def gcloud_identity_token() -> str:
    """Mirrors the deploy-cheatsheet snippet for IAP bypass against
    the direct Cloud Run URL."""
    out = subprocess.run(
        ["gcloud", "auth", "print-identity-token"],
        capture_output=True, text=True, check=True,
    )
    return out.stdout.strip()


def build_multipart(receipts: list[tuple[str, Path]]) -> tuple[bytes, str]:
    """Build a multipart/form-data body with N (file_<i>, kind_<i>)
    pairs + the standard FA fieldset. Returns (body, content_type)."""
    boundary = f"----prodtest{uuid.uuid4().hex}"
    parts: list[bytes] = []
    fields = {
        "fa_payee_name": "Prod Test",
        "fa_payee_sunet": "prodtest",
        "fa_payee_affiliation": "stanford_faculty",
        "fa_event_name": "Multi-receipt prod test",
        "fa_authorized_by": "advisor@stanford.edu",
        "fa_rush_processing": "no",
        "fa_payment_method": "electronic",
        "fa_bp_when_from": "2023-01-05",
        "fa_bp_when_to": "2023-01-09",
        "fa_bp_who": "Prod Test",
        "fa_bp_what": "Validating multi-receipt resilience on prod",
        "fa_bp_where": "San Jose, CA, USA",
        "fa_bp_why": "Engineering test",
    }
    for k, v in fields.items():
        parts.append(f"--{boundary}\r\n"
                     f'Content-Disposition: form-data; name="{k}"\r\n\r\n'
                     f"{v}\r\n".encode("utf-8"))
    for i, (kind, path) in enumerate(receipts):
        parts.append(f"--{boundary}\r\n"
                     f'Content-Disposition: form-data; name="kind_{i}"\r\n\r\n'
                     f"{kind}\r\n".encode("utf-8"))
        parts.append(f"--{boundary}\r\n"
                     f'Content-Disposition: form-data; name="file_{i}"; filename="{path.name}"\r\n'
                     f"Content-Type: application/octet-stream\r\n\r\n".encode("utf-8"))
        parts.append(path.read_bytes())
        parts.append(b"\r\n")
    parts.append(f"--{boundary}--\r\n".encode("utf-8"))
    body = b"".join(parts)
    return body, f"multipart/form-data; boundary={boundary}"


def post_upload(base_url: str, body: bytes, content_type: str,
                token: str | None) -> str:
    """POST the upload + return the upload_id from the 303 redirect.
    Handles IAP-bypass headers when token is provided."""
    headers = {"Content-Type": content_type}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(
        f"{base_url}/upload", data=body, method="POST", headers=headers,
    )
    with urllib.request.urlopen(req, timeout=60) as resp:
        final_url = resp.geturl()
    if "/upload/status/" not in final_url:
        raise RuntimeError(f"unexpected redirect: {final_url}")
    return final_url.rsplit("/", 1)[-1]


def poll_sse(base_url: str, upload_id: str, token: str | None,
             max_wait: int = 900) -> dict:
    """Subscribe to /upload/progress/<id> and return the final snapshot.
    Streams phase updates to stdout. 15-min cap for 6-receipt batches."""
    url = f"{base_url}/upload/progress/{upload_id}"
    headers = {}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    deadline = time.time() + max_wait
    last_snap: dict = {}
    while time.time() < deadline:
        try:
            req = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(req, timeout=120) as r:
                for raw in r:
                    line = raw.decode("utf-8", "replace").strip()
                    if not line.startswith("data:"):
                        continue
                    try:
                        snap = json.loads(line[len("data:"):].strip())
                    except json.JSONDecodeError:
                        continue
                    last_snap = snap
                    files = snap.get("files") or []
                    file_summary = " ".join(
                        f"{f.get('name','?')[:10]}={(f.get('status') or '?')[0]}"
                        for f in files
                    )
                    print(f"  [{time.strftime('%H:%M:%S')}] phase={snap.get('phase')}  {file_summary}",
                          flush=True)
                    if snap.get("phase") in ("done", "error", "not_found", "lost"):
                        return last_snap
        except (urllib.error.URLError, TimeoutError, OSError) as e:
            print(f"  SSE reconnect after: {e!r}", file=sys.stderr)
            time.sleep(1)
    return last_snap


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL)
    parser.add_argument(
        "--receipt", action="append",
        help="kind:path (e.g. 'meal:receipts/foo.pdf'). Repeatable. "
             "If omitted, uses 6 default receipts.",
    )
    parser.add_argument("--no-iap-bypass", action="store_true",
                        help="Skip gcloud identity token; for local Flask tests.")
    args = parser.parse_args()

    if args.receipt:
        receipts: list[tuple[str, Path]] = []
        for spec in args.receipt:
            if ":" not in spec:
                print(f"error: --receipt expects kind:path, got {spec!r}", file=sys.stderr)
                return 2
            kind, p = spec.split(":", 1)
            receipts.append((kind, Path(p)))
    else:
        receipts = [(k, RECEIPTS_DIR / f) for k, f in DEFAULT_RECEIPTS]
    for kind, p in receipts:
        if not p.exists():
            print(f"error: {p} not found", file=sys.stderr)
            return 2

    token = None
    if not args.no_iap_bypass and args.base_url.startswith("https://"):
        print("# fetching gcloud identity token for IAP bypass", flush=True)
        token = gcloud_identity_token()

    print(f"# uploading {len(receipts)} receipt(s) to {args.base_url}")
    for i, (k, p) in enumerate(receipts):
        size_kb = p.stat().st_size // 1024
        print(f"#   file_{i} ({k}): {p.name}  [{size_kb} KB]")

    body, content_type = build_multipart(receipts)
    print(f"# total multipart body: {len(body) // 1024} KB")

    start = time.time()
    print(f"# POST /upload at {time.strftime('%H:%M:%S')}")
    upload_id = post_upload(args.base_url, body, content_type, token)
    print(f"# upload_id={upload_id}")

    final = poll_sse(args.base_url, upload_id, token)
    elapsed = time.time() - start

    print(f"\n# WALLCLOCK: {elapsed:.1f}s  ({elapsed / max(1, len(receipts)):.1f}s per receipt)")
    print(f"# final phase: {final.get('phase')}")
    files = final.get("files") or []
    done = sum(1 for f in files if f.get("status") == "done")
    failed = sum(1 for f in files if f.get("status") == "failed")
    print(f"# outcomes: {done}/{len(files)} done, {failed}/{len(files)} failed")
    for f in files:
        s = f.get("status", "?")
        marker = {"done": "✓", "failed": "✗", "extracting": "…"}.get(s, "•")
        line = f"  {marker} {f.get('name', '?')}  status={s}"
        err = f.get("error")
        if err:
            line += f"\n      → {err[:200]}"
        print(line)

    if final.get("phase") == "done" and failed == 0:
        return 0
    if final.get("phase") == "done":
        return 0  # partial success — Stage 11c isolation worked, batch shipped
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
