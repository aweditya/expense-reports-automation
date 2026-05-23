"""Unified test harness for the upload pipeline. Replaces three earlier
single-purpose scripts (sse_wait_for_phase, stress_test_concurrency,
test_prod_multi_receipt) — they all overlapped on HTTP/SSE plumbing.

Three modes:

  parallel  N concurrent uploads, 1 file each. Tests concurrent FA
            usage (e.g., 3 FAs uploading simultaneously). Used as a
            release-readiness gate after any Flask threading / JOBS edit.

  batch     1 upload with N files. Tests the FA's typical multi-receipt
            batch flow (Stage 11c per-file isolation, total wallclock,
            extract phase progression).

  watch     Subscribe to an existing /upload/progress/<id> stream and
            block until phase=done. Used by ad-hoc eyeballs without
            starting a new upload.

Auth: for https://*.run.app URLs, fetches a gcloud identity token to
bypass IAP (deploy-cheatsheet pattern). Pass --no-iap-bypass for local
Flask runs that don't need it.

Cost: each receipt = ~$0.10 in Gemini calls; parallel mode = N × $0.10;
batch mode = (sum of files) × $0.10. Run sparingly.

Usage:
  scripts/test_pipeline.py parallel --n 3
  scripts/test_pipeline.py batch --n 6
  scripts/test_pipeline.py batch --receipt meal:receipts/foo.pdf --receipt airfare:receipts/bar.pdf
  scripts/test_pipeline.py watch 2026-05-22_12-34-56_abcd1234
  scripts/test_pipeline.py batch --base-url http://127.0.0.1:8765 --no-iap-bypass
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RECEIPTS_DIR = REPO_ROOT / "receipts"
DEFAULT_BASE_URL = "https://expense-reports-wgnivgelea-uw.a.run.app"

DEFAULT_FA_FIELDS = {
    "fa_payee_name": "Pipeline Test",
    "fa_payee_sunet": "pipelinetest",
    "fa_payee_affiliation": "stanford_faculty",
    "fa_event_name": "Pipeline test",
    "fa_authorized_by": "advisor@stanford.edu",
    "fa_rush_processing": "no",
    "fa_payment_method": "electronic",
    "fa_bp_when_from": "2024-09-01",
    "fa_bp_who": "Pipeline Test",
    "fa_bp_what": "Pipeline test",
    "fa_bp_where": "San Jose, CA, USA",
    "fa_bp_why": "engineering test",
}

DEFAULT_BATCH_RECEIPTS: list[tuple[str, str]] = [
    ("airfare", "airfare_2026-03-21_egencia-united-sfo-pit-roundtrip.pdf"),
    ("lodging", "lodging_2023-01-09_warren-hotel-v2.pdf"),
    ("lodging", "lodging_2023-04-16_apa-hotel-kanda-via-agoda.pdf"),
    ("transport", "transport_2023-01-05_lyft-las-vegas-strip-to-paradise.pdf"),
    ("transport", "transport_2023-01-08_lyft-dtw-to-novi.pdf"),
    ("meal", "meal_2026-03-22_pittsburgh-dinner.pdf"),
]


# ─── Shared HTTP / SSE helpers ────────────────────────────────────────────

def gcloud_identity_token() -> str:
    out = subprocess.run(
        ["gcloud", "auth", "print-identity-token"],
        capture_output=True, text=True, check=True,
    )
    return out.stdout.strip()


def auth_headers(token: str | None) -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"} if token else {}


def build_multipart(
    receipts: list[tuple[str, Path]],
    fa_fields: dict[str, str],
) -> tuple[bytes, str]:
    """Build a multipart/form-data body with the FA fieldset + N
    (file_<i>, kind_<i>) pairs. Returns (body, content_type)."""
    boundary = f"----pl{uuid.uuid4().hex}"
    parts: list[bytes] = []
    for k, v in fa_fields.items():
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
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


def post_upload(base_url: str, body: bytes, content_type: str,
                token: str | None) -> str:
    """POST /upload, follow 303, return upload_id from the final URL."""
    headers = {"Content-Type": content_type, **auth_headers(token)}
    req = urllib.request.Request(
        f"{base_url}/upload", data=body, method="POST", headers=headers,
    )
    with urllib.request.urlopen(req, timeout=60) as resp:
        final_url = resp.geturl()
    if "/upload/status/" not in final_url:
        raise RuntimeError(f"unexpected redirect: {final_url}")
    return final_url.rsplit("/", 1)[-1]


def poll_sse(base_url: str, upload_id: str, token: str | None,
             max_wait: int = 900, verbose: bool = True) -> dict:
    """Subscribe to /upload/progress/<id>; return final snapshot when
    phase reaches a terminal state (done/error/not_found/lost)."""
    url = f"{base_url}/upload/progress/{upload_id}"
    deadline = time.time() + max_wait
    last_snap: dict = {}
    while time.time() < deadline:
        try:
            req = urllib.request.Request(url, headers=auth_headers(token))
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
                    if verbose:
                        files = snap.get("files") or []
                        file_summary = " ".join(
                            f"{(f.get('name') or '?')[:14]}={(f.get('status') or '?')[0]}"
                            for f in files
                        )
                        print(f"  [{time.strftime('%H:%M:%S')}] "
                              f"phase={snap.get('phase')}  {file_summary}",
                              flush=True)
                    if snap.get("phase") in ("done", "error", "not_found", "lost"):
                        return last_snap
        except (urllib.error.URLError, TimeoutError, OSError) as e:
            print(f"  SSE reconnect after: {e!r}", file=sys.stderr)
            time.sleep(1)
    return last_snap


def print_outcomes(snapshot: dict, label: str = "") -> int:
    """Print per-file outcomes. Returns # of failed files."""
    if label:
        print(f"\n# {label}")
    files = snapshot.get("files") or []
    done = sum(1 for f in files if f.get("status") == "done")
    failed = sum(1 for f in files if f.get("status") == "failed")
    print(f"# final phase: {snapshot.get('phase')}  "
          f"outcomes: {done}/{len(files)} done, {failed}/{len(files)} failed")
    for f in files:
        s = f.get("status", "?")
        marker = {"done": "✓", "failed": "✗", "extracting": "…"}.get(s, "•")
        line = f"  {marker} {f.get('name', '?')}  status={s}"
        err = f.get("error")
        if err:
            line += f"\n      → {err[:200]}"
        print(line)
    return failed


# ─── Mode: parallel ───────────────────────────────────────────────────────

def parallel_worker(base_url: str, receipt: Path, label: str,
                    token: str | None, fa_fields: dict[str, str],
                    results: list) -> None:
    """One concurrent worker: POST one upload, watch it to completion.
    Mutates results list with outcome dict."""
    start = time.time()
    body, ct = build_multipart([("airfare", receipt)],
                               {**fa_fields, "fa_payee_name": f"PT-{label}"})
    try:
        upload_id = post_upload(base_url, body, ct, token)
    except Exception as e:
        results.append({"label": label, "ok": False, "error": f"POST: {e!r}"})
        return
    snap = poll_sse(base_url, upload_id, token, max_wait=600, verbose=False)
    elapsed = time.time() - start
    ok = snap.get("phase") == "done"
    results.append({
        "label": label, "ok": ok, "upload_id": upload_id,
        "elapsed": elapsed,
        "error": None if ok else f"phase={snap.get('phase')}",
    })


def cmd_parallel(args: argparse.Namespace, token: str | None) -> int:
    if args.receipt:
        receipts = [Path(p) for p in args.receipt]
    else:
        receipts = sorted(RECEIPTS_DIR.glob("airfare_*.pdf"))[:args.n]
    if len(receipts) < args.n:
        print(f"error: need {args.n} airfare receipts, only found {len(receipts)}",
              file=sys.stderr)
        return 2

    print(f"# parallel mode: {args.n} concurrent uploads to {args.base_url}")
    for i, r in enumerate(receipts):
        print(f"#   upload {chr(ord('A') + i)}: {r.name}")

    results: list = []
    threads: list[threading.Thread] = []
    start = time.time()
    for i in range(args.n):
        label = chr(ord("A") + i)
        t = threading.Thread(
            target=parallel_worker,
            args=(args.base_url, receipts[i], label, token, DEFAULT_FA_FIELDS, results),
            name=f"parallel-{label}",
            daemon=True,
        )
        threads.append(t)
        t.start()
    for t in threads:
        t.join()
    total = time.time() - start

    failures = 0
    print(f"\n# wallclock: {total:.1f}s")
    for r in sorted(results, key=lambda r: r.get("label", "")):
        if r.get("ok"):
            print(f"  ✓ {r['label']}  upload_id={r['upload_id']}  elapsed={r['elapsed']:.1f}s")
        else:
            print(f"  ✗ {r['label']}  error={r.get('error')}")
            failures += 1
    return 1 if failures else 0


# ─── Mode: batch ──────────────────────────────────────────────────────────

def cmd_batch(args: argparse.Namespace, token: str | None) -> int:
    if args.receipt:
        receipts: list[tuple[str, Path]] = []
        for spec in args.receipt:
            if ":" not in spec:
                print(f"error: --receipt expects kind:path, got {spec!r}", file=sys.stderr)
                return 2
            kind, p = spec.split(":", 1)
            receipts.append((kind, Path(p)))
    else:
        picks = DEFAULT_BATCH_RECEIPTS[:args.n] if args.n else DEFAULT_BATCH_RECEIPTS
        receipts = [(k, RECEIPTS_DIR / f) for k, f in picks]
    for kind, p in receipts:
        if not p.exists():
            print(f"error: {p} not found", file=sys.stderr)
            return 2

    print(f"# batch mode: 1 upload with {len(receipts)} files to {args.base_url}")
    for i, (k, p) in enumerate(receipts):
        size_kb = p.stat().st_size // 1024
        print(f"#   file_{i} ({k}): {p.name}  [{size_kb} KB]")

    body, ct = build_multipart(receipts, DEFAULT_FA_FIELDS)
    print(f"# multipart body: {len(body) // 1024} KB")

    start = time.time()
    print(f"# POST at {time.strftime('%H:%M:%S')}")
    upload_id = post_upload(args.base_url, body, ct, token)
    print(f"# upload_id={upload_id}")

    final = poll_sse(args.base_url, upload_id, token, max_wait=900, verbose=True)
    elapsed = time.time() - start
    print(f"\n# WALLCLOCK: {elapsed:.1f}s  ({elapsed / max(1, len(receipts)):.1f}s/receipt)")
    failed = print_outcomes(final)
    return 0 if final.get("phase") == "done" else 1


# ─── Mode: watch ──────────────────────────────────────────────────────────

def cmd_watch(args: argparse.Namespace, token: str | None) -> int:
    print(f"# watching {args.upload_id} on {args.base_url}")
    final = poll_sse(args.base_url, args.upload_id, token, verbose=True)
    print_outcomes(final)
    return 0 if final.get("phase") == "done" else 1


# ─── CLI ──────────────────────────────────────────────────────────────────

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL,
                        help=f"Flask base URL (default: {DEFAULT_BASE_URL})")
    parser.add_argument("--no-iap-bypass", action="store_true",
                        help="Skip gcloud identity token (for local Flask)")
    subs = parser.add_subparsers(dest="mode", required=True)

    p_par = subs.add_parser("parallel", help="N concurrent uploads, 1 file each")
    p_par.add_argument("--n", type=int, default=3)
    p_par.add_argument("--receipt", action="append",
                       help="Path to a receipt PDF (repeat for N distinct files)")

    p_bat = subs.add_parser("batch", help="1 upload with N files")
    p_bat.add_argument("--n", type=int, default=0,
                       help="Pick first N from DEFAULT_BATCH_RECEIPTS (default: all)")
    p_bat.add_argument("--receipt", action="append",
                       help="kind:path pairs (overrides DEFAULT_BATCH_RECEIPTS)")

    p_wat = subs.add_parser("watch", help="Poll an existing upload's SSE")
    p_wat.add_argument("upload_id")

    args = parser.parse_args()

    token = None
    if not args.no_iap_bypass and args.base_url.startswith("https://"):
        print("# fetching gcloud identity token (IAP bypass)")
        token = gcloud_identity_token()

    if args.mode == "parallel":
        return cmd_parallel(args, token)
    if args.mode == "batch":
        return cmd_batch(args, token)
    if args.mode == "watch":
        return cmd_watch(args, token)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
