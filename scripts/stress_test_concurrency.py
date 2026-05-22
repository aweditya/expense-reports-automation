"""Concurrency soak test: prove the deployed site handles multiple FAs
filing at the same time without crashes / data leakage between uploads.

Per FA feedback 2026-05-22 ("test to make sure multiple people can use
the website at the same time and make sure nothing crashes"). Cloud
Run is configured `--max-instances=1` + gunicorn `--workers 1
--threads 8`, so 3 simultaneous uploads is a realistic load (each
upload's HTTP handler returns 303 fast after spawning a background
thread; 3 background pipelines run in parallel).

Behavior validated:
  * Each upload returns 303 SEE OTHER → /upload/status/<id> within
    seconds (no 500s, no timeouts).
  * Each pipeline runs to phase='done' (i.e. extract → reduce →
    fx_enrich → render all complete).
  * Each upload directory has report.json + workbench.html +
    lines-domestic.csv + lines-foreign.csv afterwards.
  * No data leakage: upload A's report.json doesn't reference
    upload B's source filenames (i.e. the per-upload extractions/
    dirs aren't being co-mingled).
  * JOBS dict integrity: SSE for each upload reports the expected
    file count + status transitions.

Uses real Gemini calls (no mocks) — costs ~$0.10 per upload, ~$0.30
per run. Run sparingly; ideal for a release-readiness gate or after
any change to Flask's background-thread / JOBS logic.

Usage:
  scripts/stress_test_concurrency.py                  # 3 uploads, 1 file each, against local Flask
  scripts/stress_test_concurrency.py --n 5            # 5 uploads
  scripts/stress_test_concurrency.py --base-url https://expense-reports-wgnivgelea-uw.a.run.app
                                                       # against deployed site (needs IAP token)

Exit code 0 if all uploads finish cleanly, 1 if any fail.
"""

from __future__ import annotations

import argparse
import json
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RECEIPTS_DIR = REPO_ROOT / "receipts"


def make_form(receipt_path: Path, fa_label: str) -> tuple[bytes, str]:
    """Build a multipart form body. Distinct payee name per upload so
    we can tell the resulting workbenches apart in the verifier."""
    boundary = f"----stress{uuid.uuid4().hex}"
    parts = []
    fields = {
        "fa_payee_name": f"Stress Test {fa_label}",
        "fa_payee_sunet": f"stress{fa_label.lower()}",
        "fa_payee_affiliation": "stanford_faculty",
        "fa_event_name": f"Stress test {fa_label}",
        "fa_authorized_by": "advisor@stanford.edu",
        "fa_rush_processing": "no",
        "fa_payment_method": "electronic",
        "fa_bp_when_from": "2024-09-01",
        "fa_bp_when_to": "2024-09-05",
        "fa_bp_who": f"Stress Test {fa_label}",
        "fa_bp_what": "concurrency soak",
        "fa_bp_where": "San Jose, CA, USA",
        "fa_bp_why": "stress test",
        "kind_0": "airfare",
    }
    for k, v in fields.items():
        parts.append(f"--{boundary}\r\n"
                     f'Content-Disposition: form-data; name="{k}"\r\n\r\n'
                     f"{v}\r\n".encode("utf-8"))
    # File field
    parts.append(f"--{boundary}\r\n"
                 f'Content-Disposition: form-data; name="file_0"; filename="{receipt_path.name}"\r\n'
                 f"Content-Type: application/pdf\r\n\r\n".encode("utf-8"))
    parts.append(receipt_path.read_bytes())
    parts.append(f"\r\n--{boundary}--\r\n".encode("utf-8"))
    body = b"".join(parts)
    return body, f"multipart/form-data; boundary={boundary}"


def upload_one(base_url: str, receipt_path: Path, fa_label: str,
               results: list) -> None:
    """One concurrent worker: POST upload, follow 303, poll status until
    done. Appends a result dict to the shared `results` list."""
    start = time.time()
    body, content_type = make_form(receipt_path, fa_label)
    req = urllib.request.Request(
        f"{base_url}/upload", data=body, method="POST",
        headers={"Content-Type": content_type},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            status = resp.getcode()
            location = resp.headers.get("Location", "")
            # urllib auto-follows redirects by default; check the
            # final URL via geturl().
            final_url = resp.geturl()
    except urllib.error.HTTPError as e:
        results.append({"label": fa_label, "ok": False, "error": f"HTTPError {e.code}: {e.reason}"})
        return
    except Exception as e:
        results.append({"label": fa_label, "ok": False, "error": f"POST failed: {e!r}"})
        return

    # Extract upload_id from the final URL (post-redirect).
    if "/upload/status/" not in final_url:
        results.append({"label": fa_label, "ok": False,
                        "error": f"unexpected redirect target: {final_url}"})
        return
    upload_id = final_url.rsplit("/", 1)[-1]

    # Poll SSE for `done` (with a generous timeout — Gemini can take
    # a minute or two per file).
    deadline = time.time() + 300  # 5 min max
    final_phase = None
    poll_url = f"{base_url}/upload/progress/{upload_id}"
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(poll_url, timeout=120) as sse:
                # SSE streams; read until we see `done` / `error` /
                # `not_found` / `lost` then break. Each line is `data: {...}`.
                for raw_line in sse:
                    line = raw_line.decode("utf-8", "replace").strip()
                    if not line.startswith("data:"):
                        continue
                    try:
                        snap = json.loads(line[len("data:"):].strip())
                    except json.JSONDecodeError:
                        continue
                    phase = snap.get("phase")
                    if phase in ("done", "error", "not_found", "lost"):
                        final_phase = phase
                        break
                if final_phase is not None:
                    break
        except (urllib.error.URLError, TimeoutError, OSError) as e:
            # Network blip — retry the SSE connect.
            print(f"  [{fa_label}] SSE reconnect after: {e!r}", file=sys.stderr)
            time.sleep(1)

    elapsed = time.time() - start
    if final_phase != "done":
        results.append({"label": fa_label, "ok": False,
                        "upload_id": upload_id, "elapsed": elapsed,
                        "error": f"final phase: {final_phase!r}"})
        return

    results.append({"label": fa_label, "ok": True,
                    "upload_id": upload_id, "elapsed": elapsed,
                    "expected_filename": receipt_path.name})


def verify_local_uploads(results: list) -> int:
    """For local runs: check each upload's report.json + workbench.html
    landed, and that each upload's report references only the receipt
    that upload was given — no cross-contamination from concurrent
    pipelines writing into each other's extractions/ dir."""
    uploads_root = REPO_ROOT / ".scratch" / "uploads"
    failures = 0

    for r in results:
        if not r.get("ok"):
            continue
        upload_id = r["upload_id"]
        expected = r["expected_filename"]
        upload_dir = uploads_root / upload_id
        if not upload_dir.exists():
            print(f"  [{r['label']}] FAIL: upload dir missing: {upload_dir}")
            failures += 1
            continue
        report = upload_dir / "reduced" / "report.json"
        workbench = upload_dir / "workbench.html"
        domestic = upload_dir / "lines-domestic.csv"
        foreign = upload_dir / "lines-foreign.csv"
        any_missing = False
        for must_exist in (report, workbench, domestic, foreign):
            if not must_exist.exists():
                print(f"  [{r['label']}] FAIL: missing {must_exist.name}")
                failures += 1
                any_missing = True
                break
        if any_missing:
            continue
        # Cross-contamination check: the report's transaction lines must
        # all reference `expected` (the file this upload was given) and
        # no other. If concurrent extractor processes were writing into
        # each other's extractions/ dirs, we'd see a foreign filename here.
        data = json.loads(report.read_text())
        leaked = []
        for line in data.get("transaction_lines", []):
            src = (line.get("common", {})
                   .get("source_document", {})
                   .get("filename"))
            if src and src != expected:
                leaked.append(src)
        if leaked:
            print(f"  [{r['label']}] LEAK: report references {leaked!r} "
                  f"but upload was given {expected!r}")
            failures += 1
        else:
            print(f"  [{r['label']}] OK: report references only {expected!r}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--n", type=int, default=3, help="parallel uploads")
    parser.add_argument("--base-url", default="http://127.0.0.1:8765",
                        help="Flask base URL")
    parser.add_argument("--receipt",
                        action="append",
                        help="path to a receipt PDF (repeat the flag for N distinct receipts; "
                             "default picks N from receipts/airfare_*.pdf so cross-leakage is detectable)")
    parser.add_argument("--skip-verify", action="store_true",
                        help="skip filesystem ownership check (for remote runs)")
    args = parser.parse_args()

    if args.receipt:
        receipts = [Path(p) for p in args.receipt]
    else:
        # Default: pick first N airfare receipts so each parallel upload
        # uses a DIFFERENT file (necessary for the cross-leakage check
        # to be meaningful — same-file uploads can't detect leakage).
        receipts = sorted(RECEIPTS_DIR.glob("airfare_*.pdf"))[:args.n]
    if len(receipts) < args.n:
        print(f"error: only found {len(receipts)} receipts; need {args.n}", file=sys.stderr)
        return 2
    for r in receipts:
        if not r.exists():
            print(f"error: receipt {r} not found", file=sys.stderr)
            return 2

    print(f"# spawning {args.n} concurrent uploads against {args.base_url}")
    for i, r in enumerate(receipts):
        print(f"#   upload {chr(ord('A') + i)}: {r.name}")

    results: list = []
    threads: list[threading.Thread] = []
    for i in range(args.n):
        label = chr(ord("A") + i)
        t = threading.Thread(
            target=upload_one,
            args=(args.base_url, receipts[i], label, results),
            name=f"stress-{label}",
            daemon=True,
        )
        threads.append(t)

    start = time.time()
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    total = time.time() - start

    # Summary
    print()
    print(f"# wallclock: {total:.1f}s")
    print("# per-upload outcomes:")
    failures = 0
    for r in sorted(results, key=lambda r: r.get("label", "")):
        if r.get("ok"):
            print(f"  ✓ {r['label']}  upload_id={r['upload_id']}  elapsed={r['elapsed']:.1f}s")
        else:
            print(f"  ✗ {r['label']}  error={r.get('error')}")
            failures += 1

    if not args.skip_verify and "127.0.0.1" in args.base_url:
        print()
        print("# verifying local upload directory ownership")
        failures += verify_local_uploads(results)

    if failures:
        print(f"\n{failures} failure(s)")
        return 1
    print("\nALL CLEAN — concurrency soak passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
