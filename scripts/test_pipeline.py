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


# ─── Mode: ui-batch ───────────────────────────────────────────────────────

# Default 5-receipt mix for ui-batch — diverse kinds covering the main
# extractor paths. Smaller than DEFAULT_BATCH_RECEIPTS because ui-batch
# is meant for routine per-deploy validation rather than full-load
# stress; the CLI batch mode covers high-N stress.
UI_BATCH_DEFAULT: list[tuple[str, str]] = [
    ("airfare", "airfare_2026-03-21_egencia-united-sfo-pit-roundtrip.pdf"),
    ("lodging", "lodging_2023-01-09_warren-hotel-v2.pdf"),
    ("transport", "transport_2023-01-05_lyft-las-vegas-strip-to-paradise.pdf"),
    ("meal", "meal_2026-03-21_southern-tier-pittsburgh.pdf"),
    ("membership", "membership_2026-01-29_acm-student-renewal.png"),
]


def cmd_ui_batch(args: argparse.Namespace, token: str | None) -> int:
    """Drive the FA upload form via Playwright + verify the full
    browser-side flow: form fill → file inputs → submit → progress
    page SSE → redirect to workbench → workbench renders all lines.

    Catches regressions the CLI batch mode misses:
      - Form rendering at N inputs (the +Add another file button)
      - localStorage interaction under load
      - Browser-side EventSource handling a multi-minute SSE stream
      - Workbench DOM size at N lines (rendering perf, scroll)

    Saves screenshots to .scratch/ui-stress-N{N}/ for visual eyeball.

    Cost: ~$0.10/receipt in Gemini calls. Default N=5 ≈ $0.50."""
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        print("error: playwright not installed; run "
              "./.venv/bin/pip install -r dev-requirements.txt && "
              "./.venv/bin/playwright install chromium", file=sys.stderr)
        return 3

    # Pick the receipt list. If --receipt given, use those. Else cycle
    # through UI_BATCH_DEFAULT up to N (repeating if N > len).
    if args.receipt:
        picks: list[tuple[str, Path]] = []
        for spec in args.receipt:
            if ":" not in spec:
                print(f"error: --receipt expects kind:path, got {spec!r}",
                      file=sys.stderr)
                return 2
            kind, p = spec.split(":", 1)
            picks.append((kind, Path(p)))
    else:
        n = args.n if args.n > 0 else len(UI_BATCH_DEFAULT)
        picks = [
            (UI_BATCH_DEFAULT[i % len(UI_BATCH_DEFAULT)][0],
             RECEIPTS_DIR / UI_BATCH_DEFAULT[i % len(UI_BATCH_DEFAULT)][1])
            for i in range(n)
        ]
    for kind, p in picks:
        if not p.exists():
            print(f"error: {p} not found", file=sys.stderr)
            return 2

    n = len(picks)
    out_dir = REPO_ROOT / ".scratch" / f"ui-stress-N{n}"
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"# ui-batch mode: 1 upload with {n} files via real browser "
          f"to {args.base_url}")
    for i, (k, p) in enumerate(picks):
        size_kb = p.stat().st_size // 1024
        print(f"#   file_{i} ({k}): {p.name}  [{size_kb} KB]")
    print(f"# screenshots → {out_dir}")

    extra_headers = {}
    if token:
        # Chromium rejects setting Proxy-Authorization via
        # extra_http_headers (it's a reserved proxy header). The
        # standard Authorization: Bearer header is the working path
        # for IAP-protected Cloud Run hosts and matches what the CLI
        # mode uses for raw urllib requests (see auth_headers()).
        extra_headers["Authorization"] = f"Bearer {token}"

    start = time.time()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        ctx = browser.new_context(
            viewport={"width": 1400, "height": 1800},
            extra_http_headers=extra_headers,
        )
        page = ctx.new_page()

        console_errors: list[str] = []
        page.on("console", lambda msg: (
            console_errors.append(msg.text) if msg.type == "error" else None))

        # 1. Load the upload form.
        page.goto(args.base_url + "/", wait_until="domcontentloaded")
        page.screenshot(path=str(out_dir / "01-form-blank.png"))

        # 2. Fill the FA fieldset.
        for name, value in DEFAULT_FA_FIELDS.items():
            sel = f'[name="{name}"]'
            el = page.query_selector(sel)
            if el is None:
                print(f"warning: form field {name} not found", file=sys.stderr)
                continue
            tag = el.evaluate("e => e.tagName")
            if tag == "SELECT":
                page.select_option(sel, value)
            else:
                page.fill(sel, value)

        # 3. Add N-1 more file rows via the + Add another file button.
        for _ in range(n - 1):
            page.click('button.row-add, #add-file-button, [onclick*="addFileRow"]')

        # 4. Set files + select kinds. set_input_files takes the
        # absolute path; the form uses indexed names file_0/kind_0/...
        for i, (kind, p) in enumerate(picks):
            page.set_input_files(f'[name="file_{i}"]', str(p.resolve()))
            page.select_option(f'[name="kind_{i}"]', kind)

        page.screenshot(path=str(out_dir / "02-form-filled.png"), full_page=True)

        # 5. Submit. The form does POST-redirect-GET to /upload/status/<id>.
        with page.expect_navigation(wait_until="domcontentloaded",
                                    timeout=60_000):
            page.click('button[type="submit"], input[type="submit"]')
        upload_id = page.url.rstrip("/").split("/")[-1]
        print(f"# upload_id={upload_id}  (after {time.time()-start:.1f}s)")

        # 6. Wait briefly + screenshot mid-extract.
        page.wait_for_timeout(30_000)
        page.screenshot(path=str(out_dir / "03-progress-mid.png"))

        # 7. Wait for the JS-driven redirect to workbench (up to 15min).
        try:
            page.wait_for_url("**/workbench.html", timeout=900_000)
        except Exception as err:
            page.screenshot(path=str(out_dir / "99-stuck-progress.png"))
            print(f"error: never reached workbench: {err}", file=sys.stderr)
            browser.close()
            return 1

        # 8. Workbench loaded. Wait for network idle so the JS finishes.
        page.wait_for_load_state("networkidle", timeout=30_000)
        page.screenshot(path=str(out_dir / "04-workbench-done.png"),
                        full_page=True)

        # 9. Assertions: N line cards, no console errors during browse.
        lines = page.query_selector_all("details.line-card")
        cards = page.query_selector_all("[data-path]")
        wallclock = time.time() - start
        print(f"\n# WALLCLOCK: {wallclock:.1f}s  ({wallclock / n:.1f}s/receipt)")
        print(f"# workbench: {len(lines)} line cards, "
              f"{len(cards)} editable field cards")

        ok = True
        if len(lines) != n:
            print(f"FAIL: expected {n} line cards, got {len(lines)}",
                  file=sys.stderr)
            ok = False
        if console_errors:
            print(f"WARN: {len(console_errors)} console errors:",
                  file=sys.stderr)
            for e in console_errors[:5]:
                print(f"  {e}", file=sys.stderr)
        browser.close()
    return 0 if ok else 1


# ─── Mode: ui-parallel ────────────────────────────────────────────────────

def cmd_ui_parallel(args: argparse.Namespace, token: str | None) -> int:
    """N concurrent Playwright sessions, each running an upload flow.

    Stress-tests Cloud Run's gunicorn thread pool + Firestore-backed
    JOBS for cross-session isolation + multi-FA SSE streams. Each
    session uses a fresh browser context so cookies + localStorage
    are isolated.

    Cost: ~$0.10/receipt across all sessions combined. Default 3 FAs
    with 2 receipts each = $0.60.
    """
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        print("error: playwright not installed; run "
              "./.venv/bin/pip install -r dev-requirements.txt && "
              "./.venv/bin/playwright install chromium", file=sys.stderr)
        return 3

    n_fas = args.n
    per_fa_pool = [
        ("airfare", "airfare_2026-03-21_egencia-united-sfo-pit-roundtrip.pdf"),
        ("meal", "meal_2026-03-21_southern-tier-pittsburgh.pdf"),
        ("transport", "transport_2023-01-08_lyft-dtw-to-novi.pdf"),
        ("lodging", "lodging_2023-01-09_warren-hotel-v2.pdf"),
        ("membership", "membership_2026-01-29_acm-student-renewal.png"),
        ("meal", "meal_2026-04-04_original-mels-san-leandro.jpeg"),
    ]
    for _kind, name in per_fa_pool:
        path = RECEIPTS_DIR / name
        if not path.exists():
            print(f"error: receipt not found: {path}", file=sys.stderr)
            return 2

    out_root = REPO_ROOT / ".scratch" / f"ui-parallel-N{n_fas}"
    out_root.mkdir(parents=True, exist_ok=True)
    print(f"# ui-parallel: {n_fas} concurrent browser sessions, "
          f"2 receipts each, against {args.base_url}")
    print(f"# screenshots + logs → {out_root}")

    extra_headers = {"Authorization": f"Bearer {token}"} if token else {}

    results: list[dict] = []
    results_lock = threading.Lock()

    def run_one_fa(idx: int) -> None:
        """One FA's flow. Picks 2 receipts from the pool, offset by idx
        so concurrent FAs use different files."""
        label = f"fa{idx}"
        out_dir = out_root / label
        out_dir.mkdir(parents=True, exist_ok=True)
        start = time.time()
        picks = [
            per_fa_pool[(idx * 2 + i) % len(per_fa_pool)]
            for i in range(2)
        ]
        try:
            with sync_playwright() as pw:
                browser = pw.chromium.launch(headless=True)
                ctx = browser.new_context(
                    viewport={"width": 1200, "height": 1600},
                    extra_http_headers=extra_headers,
                )
                page = ctx.new_page()
                console_errors: list[str] = []
                page.on("console", lambda msg: (
                    console_errors.append(msg.text)
                    if msg.type == "error" else None))

                page.goto(args.base_url + "/", wait_until="domcontentloaded")
                for name, value in DEFAULT_FA_FIELDS.items():
                    el = page.query_selector(f'[name="{name}"]')
                    if el is None:
                        continue
                    tag = el.evaluate("e => e.tagName")
                    if tag == "SELECT":
                        page.select_option(f'[name="{name}"]', value)
                    else:
                        page.fill(f'[name="{name}"]', value)
                # Add second file row
                page.click('button:has-text("+ Add another file")')
                for i, (kind, fname) in enumerate(picks):
                    receipt_path = RECEIPTS_DIR / fname
                    page.set_input_files(f'[name="file_{i}"]',
                                         str(receipt_path))
                    page.select_option(f'[name="kind_{i}"]', kind)
                page.screenshot(path=str(out_dir / "01-filled.png"))

                with page.expect_navigation(wait_until="domcontentloaded",
                                            timeout=60_000):
                    page.click('button[type="submit"]')
                upload_id = page.url.rstrip("/").split("/")[-1]

                page.wait_for_url("**/workbench.html", timeout=900_000)
                page.wait_for_load_state("networkidle", timeout=30_000)
                page.screenshot(path=str(out_dir / "02-workbench.png"),
                                full_page=True)
                lines = page.query_selector_all("details.line-card")
                wallclock = time.time() - start
                ok = (len(lines) == len(picks)
                      and not any(
                          "undo-available" not in e for e in console_errors))
                with results_lock:
                    results.append({
                        "label": label,
                        "upload_id": upload_id,
                        "lines": len(lines),
                        "expected": len(picks),
                        "console_errors": console_errors,
                        "elapsed_s": wallclock,
                        "ok": ok,
                    })
                browser.close()
        except Exception as exc:
            with results_lock:
                results.append({
                    "label": label,
                    "ok": False,
                    "error": str(exc),
                    "elapsed_s": time.time() - start,
                })

    start_all = time.time()
    threads = [threading.Thread(target=run_one_fa, args=(i,))
               for i in range(n_fas)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    total_wall = time.time() - start_all

    print(f"\n# WALLCLOCK: {total_wall:.1f}s")
    failures = 0
    for r in sorted(results, key=lambda x: x.get("label", "")):
        if r.get("ok"):
            print(f"  OK   {r['label']}  upload_id={r.get('upload_id')}  "
                  f"lines={r.get('lines')}/{r.get('expected')}  "
                  f"elapsed={r['elapsed_s']:.1f}s")
        else:
            print(f"  FAIL {r['label']}  "
                  f"lines={r.get('lines')}/{r.get('expected')}  "
                  f"elapsed={r['elapsed_s']:.1f}s")
            if r.get("error"):
                print(f"       exception: {r['error']}")
            errs = r.get("console_errors") or []
            for e in errs[:5]:
                print(f"       console: {e}")
            failures += 1
    return 1 if failures else 0


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

    p_ui = subs.add_parser(
        "ui-batch",
        help="Drive the FA form via Playwright + verify workbench "
             "renders all lines (browser-side stress test)")
    p_ui.add_argument("--n", type=int, default=5,
                      help="Number of receipts (default: 5; cycles "
                           "through UI_BATCH_DEFAULT)")
    p_ui.add_argument("--receipt", action="append",
                      help="kind:path pairs (overrides UI_BATCH_DEFAULT)")

    p_up = subs.add_parser(
        "ui-parallel",
        help="N concurrent Playwright sessions, each uploading 2 files")
    p_up.add_argument("--n", type=int, default=3,
                      help="Number of concurrent FA sessions (default: 3)")

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
    if args.mode == "ui-batch":
        return cmd_ui_batch(args, token)
    if args.mode == "ui-parallel":
        return cmd_ui_parallel(args, token)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
